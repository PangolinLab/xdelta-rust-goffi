// xdelta_interface.h
#ifndef XDELTA_INTERFACE_H
#define XDELTA_INTERFACE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// 返回 0 表示成功，负数表示失败。失败后可通过 xdelta_last_error() 获取错误字符串（只读指针，线程局部）。
int xdelta_create_patch_file(const char* old_path, const char* new_path, const char* patch_path, uint32_t block_size);
int xdelta_apply_patch_file(const char* old_path, const char* patch_path, const char* out_path);
const char* xdelta_last_error(void);

#ifdef __cplusplus
}
#endif

#endif // XDELTA_INTERFACE_H
