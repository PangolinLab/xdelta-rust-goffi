// xdelta_interface.h
#ifndef XDELTA_INTERFACE_H
#define XDELTA_INTERFACE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// 返回 0 表示成功，负数表示失败。失败后可通过 xdelta_last_error() 获取错误字符串（只读指针，线程局部）。
int xdelta_create_patch_data(const uint8_t* old_data, size_t old_len,
                             const uint8_t* new_data, size_t new_len,
                             uint8_t** patch_data, size_t* patch_len,
                             uint32_t block_size);
int xdelta_apply_patch_data(const uint8_t* old_data, size_t old_len,
                            const uint8_t* patch_data, size_t patch_len,
                            uint8_t** new_data, size_t* new_len);
void xdelta_free_data(uint8_t* data);
const char* xdelta_last_error(void);

#ifdef __cplusplus
}
#endif

#endif // XDELTA_INTERFACE_H
