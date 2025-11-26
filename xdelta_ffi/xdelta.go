package xdelta_ffi

/*
	#cgo CFLAGS: -I${SRCDIR}/include
	#cgo LDFLAGS: -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -L${SRCDIR}/bin -lxdelta
	#include <stdlib.h>
	#include <xdelta_interface.h>
*/
import "C"
import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"unsafe"
)

func init() {
	// 动态库最终路径
	var libFile string
	switch runtime.GOOS {
	case "windows":
		libFile = "bin/xdelta.dll"
	case "darwin":
		libFile = "bin/libxdelta.dylib"
	default:
		libFile = "bin/libxdelta.so"
	}

	// 如果库不存在，则编译 Rust 并复制到 bin/
	if _, err := os.Stat(libFile); os.IsNotExist(err) {
		// Rust 源码目录（Cargo.toml 所在目录）
		rustDir := "../" // 根据你的目录结构调整
		buildCmd := exec.Command("cargo", "build", "--release")
		buildCmd.Dir = rustDir
		buildCmd.Stdout = os.Stdout
		buildCmd.Stderr = os.Stderr
		if err := buildCmd.Run(); err != nil {
			panic("Failed to build Rust library: " + err.Error())
		}

		// 源文件路径（默认 target/release/）
		var srcLib string
		switch runtime.GOOS {
		case "windows":
			srcLib = filepath.Join(rustDir, "target", "release", "xdelta.dll")
		case "darwin":
			srcLib = filepath.Join(rustDir, "target", "release", "libxdelta.dylib")
		default:
			srcLib = filepath.Join(rustDir, "target", "release", "libxdelta.so")
		}

		// 确保 bin 目录存在
		_ = os.MkdirAll("bin", 0755)

		// 复制库到 bin/
		input, err := os.ReadFile(srcLib)
		if err != nil {
			panic("Failed to read Rust library: " + err.Error())
		}
		if err := os.WriteFile(libFile, input, 0644); err != nil {
			panic("Failed to write library to bin/: " + err.Error())
		}
	}
}

// CreatePatchFile calls the Rust library to create a patch file.
// blockSize: 4096 typical default.
func CreatePatchFile(oldPath, newPath, patchPath string, blockSize uint32) error {
	cold := C.CString(oldPath)
	cnew := C.CString(newPath)
	cpatch := C.CString(patchPath)
	defer C.free(unsafe.Pointer(cold))
	defer C.free(unsafe.Pointer(cnew))
	defer C.free(unsafe.Pointer(cpatch))

	r := C.xdelta_create_patch_file(cold, cnew, cpatch, C.uint32_t(blockSize))
	if r == 0 {
		return nil
	}
	cerr := C.xdelta_last_error()
	if cerr != nil {
		return fmt.Errorf("xdelta error: %s", C.GoString(cerr))
	}
	return fmt.Errorf("xdelta unknown error")
}

// ApplyPatchFile applies patch (patchPath) to oldPath and writes result to outPath.
func ApplyPatchFile(oldPath, patchPath, outPath string) error {
	cold := C.CString(oldPath)
	cpatch := C.CString(patchPath)
	cout := C.CString(outPath)
	defer C.free(unsafe.Pointer(cold))
	defer C.free(unsafe.Pointer(cpatch))
	defer C.free(unsafe.Pointer(cout))

	r := C.xdelta_apply_patch_file(cold, cpatch, cout)
	if r == 0 {
		return nil
	}
	cerr := C.xdelta_last_error()
	if cerr != nil {
		return fmt.Errorf("xdelta error: %s", C.GoString(cerr))
	}
	return fmt.Errorf("xdelta unknown error")
}
