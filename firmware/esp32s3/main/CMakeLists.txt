idf_component_register(
    SRCS "csi_capture.c"
    INCLUDE_DIRS "."
    REQUIRES esp_wifi nvs_flash esp_netif lwip esp_timer
)
