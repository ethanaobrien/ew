#include <android/log.h>

#define LOGI(tag, ...) __android_log_print(ANDROID_LOG_INFO, tag, __VA_ARGS__)

void android_log(const char* tag, const char* out) {
    LOGI(tag, "%s", out);
}
