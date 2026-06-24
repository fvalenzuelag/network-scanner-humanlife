/*
 * csi_capture.c — ESP32-S3 CSI capture and UDP streaming
 * Part of Network-Scanner-3D
 *
 * Connects the ESP32-S3 to a WiFi AP, enables Channel State Information (CSI)
 * capture via the esp_wifi CSI API, and streams frames over UDP to the
 * ns3d-server using the NS3D wire format.
 *
 * Wire format (matches ns3d-core/src/lib.rs `parse_udp_frame`):
 *   [magic:4] [node_id:1] [channel:1] [rssi:1] [timestamp_ms:8]
 *   [num_sub:1] [i16 re:2, i16 im:2] × num_sub
 *   Total: 16 + num_sub * 4 bytes
 *
 * Build:
 *   idf.py set-target esp32s3
 *   idf.py build
 *   idf.py -p /dev/tty.usbserial-* flash monitor
 *
 * Configuration — edit the #defines below or use Kconfig:
 */

#include <string.h>
#include <stdint.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/event_groups.h"
#include "esp_system.h"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "nvs_flash.h"
#include "lwip/sockets.h"
#include "lwip/netdb.h"

// ── User configuration ────────────────────────────────────────────────────────

#define WIFI_SSID        "YOUR_SSID"          /* <-- change me */
#define WIFI_PASS        "YOUR_PASSWORD"      /* <-- change me */
#define SERVER_IP        "192.168.0.1"        /* <-- ns3d-server IP */
#define SERVER_PORT      5006
#define NODE_ID          1                    /* 1-based, unique per ESP32 */
#define PING_TARGET_IP   SERVER_IP            /* send pings to generate CSI */
#define PING_INTERVAL_MS 10                   /* ~100 Hz CSI rate */
#define WIFI_CHANNEL     6                    /* AP channel to use */

// ── NS3D magic ────────────────────────────────────────────────────────────────

#define NS3D_MAGIC       0x4E533344u          /* "NS3D" big-endian */
#define MAX_SUBCARRIERS  56

static const char *TAG = "ns3d";

// ── Global state ──────────────────────────────────────────────────────────────

static int         g_udp_sock   = -1;
static struct sockaddr_in g_dest;
static EventGroupHandle_t g_wifi_events;
#define WIFI_CONNECTED_BIT BIT0

// ── UDP frame builder ─────────────────────────────────────────────────────────

static void send_csi_frame(const wifi_csi_info_t *info) {
    if (g_udp_sock < 0) return;

    const wifi_pkt_rx_ctrl_t *rx = &info->rx_ctrl;
    int8_t  rssi    = rx->rssi;
    uint8_t channel = rx->channel;

    /* CSI buffer: interleaved int8_t [lltf_re, lltf_im, htltf_re, htltf_im …]
       We take the LLTF (first 56 × 2 int8 values).
       Scale to int16 for wire format (multiply by 4 to use more of int16 range). */
    uint8_t  num_sub = 0;
    int16_t  re_im[MAX_SUBCARRIERS * 2];   /* [re0, im0, re1, im1, …] */

    if (info->buf && info->len >= 4) {
        num_sub = (uint8_t)((info->len / 2) < MAX_SUBCARRIERS
                            ? (info->len / 2)
                            : MAX_SUBCARRIERS);
        for (int i = 0; i < num_sub; i++) {
            re_im[i*2]     = (int16_t)info->buf[i*2]     * 4;
            re_im[i*2 + 1] = (int16_t)info->buf[i*2 + 1] * 4;
        }
    }

    uint64_t ts_ms = (uint64_t)esp_timer_get_time() / 1000;

    /* Build NS3D wire frame */
    size_t payload_len = 16 + (size_t)num_sub * 4;
    uint8_t buf[16 + MAX_SUBCARRIERS * 4];
    uint8_t *p = buf;

    /* magic (big-endian) */
    p[0] = (NS3D_MAGIC >> 24) & 0xFF;
    p[1] = (NS3D_MAGIC >> 16) & 0xFF;
    p[2] = (NS3D_MAGIC >>  8) & 0xFF;
    p[3] =  NS3D_MAGIC        & 0xFF;
    p += 4;

    *p++ = (uint8_t)NODE_ID;
    *p++ = channel;
    *p++ = (uint8_t)rssi;

    /* timestamp_ms big-endian 8 bytes */
    for (int i = 7; i >= 0; i--) { *(p++) = (ts_ms >> (i*8)) & 0xFF; }

    *p++ = num_sub;

    /* subcarrier I/Q pairs big-endian */
    for (int i = 0; i < num_sub; i++) {
        int16_t re = re_im[i*2];
        int16_t im = re_im[i*2 + 1];
        *p++ = (re >> 8) & 0xFF;
        *p++ =  re       & 0xFF;
        *p++ = (im >> 8) & 0xFF;
        *p++ =  im       & 0xFF;
    }

    sendto(g_udp_sock, buf, payload_len, 0,
           (struct sockaddr *)&g_dest, sizeof(g_dest));

    ESP_LOGD(TAG, "CSI frame: node=%d ch=%d rssi=%d sub=%d",
             NODE_ID, channel, rssi, num_sub);
}

// ── CSI callback ──────────────────────────────────────────────────────────────

static void csi_cb(void *ctx, wifi_csi_info_t *data) {
    send_csi_frame(data);
}

// ── WiFi event handler ────────────────────────────────────────────────────────

static void wifi_event_handler(void *arg, esp_event_base_t base,
                               int32_t id, void *event_data) {
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_START) {
        esp_wifi_connect();
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        ESP_LOGW(TAG, "WiFi disconnected, retrying…");
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *ev = (ip_event_got_ip_t *)event_data;
        ESP_LOGI(TAG, "Got IP: " IPSTR, IP2STR(&ev->ip_info.ip));
        xEventGroupSetBits(g_wifi_events, WIFI_CONNECTED_BIT);
    }
}

// ── WiFi init ─────────────────────────────────────────────────────────────────

static void wifi_init(void) {
    g_wifi_events = xEventGroupCreate();

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_sta();

    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));

    ESP_ERROR_CHECK(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID,
                                               wifi_event_handler, NULL));
    ESP_ERROR_CHECK(esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP,
                                               wifi_event_handler, NULL));

    wifi_config_t wcfg = {
        .sta = {
            .ssid     = WIFI_SSID,
            .password = WIFI_PASS,
            .threshold.authmode = WIFI_AUTH_WPA2_PSK,
        },
    };
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wcfg));
    ESP_ERROR_CHECK(esp_wifi_start());

    /* Wait until connected */
    xEventGroupWaitBits(g_wifi_events, WIFI_CONNECTED_BIT,
                        pdFALSE, pdTRUE, portMAX_DELAY);
    ESP_LOGI(TAG, "WiFi connected to '%s'", WIFI_SSID);
}

// ── CSI setup ─────────────────────────────────────────────────────────────────

static void csi_init(void) {
    /* Enable promiscuous to receive all 802.11 frames (needed for CSI) */
    ESP_ERROR_CHECK(esp_wifi_set_promiscuous(true));

    wifi_csi_config_t csi_cfg = {
        .lltf_en           = true,   /* Legacy Long Training Field */
        .htltf_en          = false,
        .stbc_htltf2_en    = false,
        .ltf_merge_en      = true,
        .channel_filter_en = true,
        .manu_scale        = false,
    };
    ESP_ERROR_CHECK(esp_wifi_set_csi_config(&csi_cfg));
    ESP_ERROR_CHECK(esp_wifi_set_csi_rx_cb(csi_cb, NULL));
    ESP_ERROR_CHECK(esp_wifi_set_csi(true));

    ESP_LOGI(TAG, "CSI enabled on channel %d", WIFI_CHANNEL);
}

// ── UDP socket ────────────────────────────────────────────────────────────────

static void udp_init(void) {
    g_udp_sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_IP);
    if (g_udp_sock < 0) {
        ESP_LOGE(TAG, "Cannot create UDP socket");
        return;
    }

    memset(&g_dest, 0, sizeof(g_dest));
    g_dest.sin_family = AF_INET;
    g_dest.sin_port   = htons(SERVER_PORT);
    inet_aton(SERVER_IP, &g_dest.sin_addr);

    ESP_LOGI(TAG, "UDP sink: %s:%d", SERVER_IP, SERVER_PORT);
}

// ── Ping task (generates CSI traffic) ────────────────────────────────────────

static void ping_task(void *arg) {
    /* Simple ICMP echo to SERVER_IP — keeps CSI flowing even without other
       WiFi traffic. In a real deployment you might use sniffer mode and
       rely on background beacon/multicast traffic instead. */
    struct sockaddr_in dest = g_dest;
    dest.sin_port = htons(7); /* echo port */

    int s = socket(AF_INET, SOCK_DGRAM, IPPROTO_IP);
    static const char *msg = "ns3d-ping";

    for (;;) {
        sendto(s, msg, strlen(msg), 0, (struct sockaddr *)&dest, sizeof(dest));
        vTaskDelay(pdMS_TO_TICKS(PING_INTERVAL_MS));
    }
}

// ── app_main ──────────────────────────────────────────────────────────────────

void app_main(void) {
    ESP_LOGI(TAG, "Network-Scanner-3D firmware v0.1.0");
    ESP_LOGI(TAG, "Hardware: ESP32-S3-N16R8  Node ID: %d", NODE_ID);

    /* NVS (required by WiFi driver) */
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ESP_ERROR_CHECK(nvs_flash_init());
    }

    wifi_init();
    udp_init();
    csi_init();

    /* Start ping task on core 1 (CSI callback runs on core 0) */
    xTaskCreatePinnedToCore(ping_task, "ping", 2048, NULL, 5, NULL, 1);

    ESP_LOGI(TAG, "Streaming CSI to %s:%d — open http://%s:3000",
             SERVER_IP, SERVER_PORT, SERVER_IP);
}
