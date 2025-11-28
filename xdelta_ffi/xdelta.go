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

// CreateDiffsData 从两个文件数据创建补丁数据
// 较小的 blockSize 可以提高匹配精度，但会增加计算开销
// 较大的 blockSize 会减少计算时间，但可能降低匹配效率
func CreateDiffsData(oldData, newData []byte, blockSize uint32) ([]byte, error) {
	oldPtr := (*C.uint8_t)(C.CBytes(oldData))
	newPtr := (*C.uint8_t)(C.CBytes(newData))
	defer C.free(unsafe.Pointer(oldPtr))
	defer C.free(unsafe.Pointer(newPtr))

	var patchPtr *C.uint8_t
	var patchLen C.size_t

	r := C.xdelta_create_patch_data(
		oldPtr, C.size_t(len(oldData)),
		newPtr, C.size_t(len(newData)),
		&patchPtr, &patchLen,
		C.uint32_t(blockSize),
	)

	if r != 0 {
		cerr := C.xdelta_last_error()
		if cerr != nil {
			return nil, fmt.Errorf("xdelta error: %s", C.GoString(cerr))
		}
		return nil, fmt.Errorf("xdelta unknown error")
	}

	defer C.xdelta_free_data(patchPtr)

	patchData := C.GoBytes(unsafe.Pointer(patchPtr), C.int(patchLen))
	return patchData, nil
}

// ApplyDiffsData 将补丁应用到旧数据生成新数据
func ApplyDiffsData(oldData, diffsData []byte) ([]byte, error) {
	oldPtr := (*C.uint8_t)(C.CBytes(oldData))
	patchPtr := (*C.uint8_t)(C.CBytes(diffsData))
	defer C.free(unsafe.Pointer(oldPtr))
	defer C.free(unsafe.Pointer(patchPtr))

	var newPtr *C.uint8_t
	var newLen C.size_t

	r := C.xdelta_apply_patch_data(
		oldPtr, C.size_t(len(oldData)),
		patchPtr, C.size_t(len(diffsData)),
		&newPtr, &newLen,
	)

	if r != 0 {
		cerr := C.xdelta_last_error()
		if cerr != nil {
			return nil, fmt.Errorf("xdelta error: %s", C.GoString(cerr))
		}
		return nil, fmt.Errorf("xdelta unknown error")
	}

	defer C.xdelta_free_data(newPtr)

	newData := C.GoBytes(unsafe.Pointer(newPtr), C.int(newLen))
	return newData, nil
}
