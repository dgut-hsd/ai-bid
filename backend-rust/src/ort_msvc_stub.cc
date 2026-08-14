// Stub implementations of MSVC STL internal functions that are missing
// when linking ort-sys (ONNX Runtime C++ bindings) with MSVC 14.43.
//
// These functions are defined as inline in MSVC 14.43's <xutility> header,
// but the compiler may decide to emit out-of-line calls to them. Since they
// are inline-only, no .lib exports them. We provide non-vectorized fallbacks.

#include <cstddef>

extern "C" {

// Vectorized find_first_of for char (returns position, not pointer)
size_t __std_find_first_of_trivial_pos_1(
    const char* haystack, size_t haystack_len,
    const char* needles, size_t needles_len)
{
    for (size_t i = 0; i < haystack_len; ++i) {
        for (size_t j = 0; j < needles_len; ++j) {
            if (haystack[i] == needles[j]) {
                return i;
            }
        }
    }
    return static_cast<size_t>(-1); // npos
}

// Vectorized find_first_of for wchar_t
size_t __std_find_first_of_trivial_pos_2(
    const wchar_t* haystack, size_t haystack_len,
    const wchar_t* needles, size_t needles_len)
{
    for (size_t i = 0; i < haystack_len; ++i) {
        for (size_t j = 0; j < needles_len; ++j) {
            if (haystack[i] == needles[j]) {
                return i;
            }
        }
    }
    return static_cast<size_t>(-1); // npos
}

} // extern "C"
