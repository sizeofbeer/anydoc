//! C ABI export layer (shim) over the anydoc core.
//!
//! This module exposes a small, stable `extern "C"` surface so that native
//! consumers (e.g. a C++ process loading `libanydoc_capi` via `dlopen` /
//! `LoadLibrary`) can reuse anydoc's document-to-Markdown conversion without a
//! Node runtime and without touching the deeply-nested, polymorphic
//! [`Document`](crate::model::Document) model.
//!
//! Design decisions (see the project C-ABI plan):
//! - **Markdown only** (`to_markdown`); the document model is not flattened
//!   across the boundary.
//! - **Caller-free**: strings are returned as heap pointers the caller must
//!   release with [`anydoc_string_free`].
//! - **int error codes**: `0` on success, a stable non-zero code otherwise
//!   (mirrors [`ConvertError::code`]), with the human-readable message
//!   retrievable via [`anydoc_last_error`].
//!
//! Every string crossing the boundary is UTF-8, allocated via
//! `CString::into_raw`, and owned by the caller until freed.

use crate::{ConvertError, Format};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::slice;

// ---------------------------------------------------------------------------
// Error code mapping
// ---------------------------------------------------------------------------

/// Stable error codes returned by the C ABI. `0` means success.
///
/// These mirror the values of [`ConvertError::code`]; keep them in sync.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // `Ok` is the success value exposed to C callers.
pub enum AnyDocErrorCode {
    /// Success.
    Ok = 0,
    /// The format is unknown or cannot be converted at all.
    Unsupported = 1,
    /// The document is structurally unusable.
    Malformed = 2,
    /// The document is encrypted or password-protected.
    Encrypted = 3,
    /// A fixed safety/resource limit was exceeded.
    ResourceLimit = 4,
    /// A required part or stream is missing.
    MissingPart = 5,
    /// The input could not be read.
    Io = 6,
}

fn error_code(err: &ConvertError) -> AnyDocErrorCode {
    match err.code() {
        "unsupported" => AnyDocErrorCode::Unsupported,
        "malformed" => AnyDocErrorCode::Malformed,
        "encrypted" => AnyDocErrorCode::Encrypted,
        "resourceLimit" => AnyDocErrorCode::ResourceLimit,
        "missingPart" => AnyDocErrorCode::MissingPart,
        _ => AnyDocErrorCode::Io,
    }
}

// ---------------------------------------------------------------------------
// Format code mapping
// ---------------------------------------------------------------------------

/// Stable format codes. `-1` means "unrecognized".
///
/// These mirror [`Format`] (see `src/lib.rs`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyDocFormat {
    /// Unknown / unrecognized.
    Unknown = -1,
    /// Binary Word 97-2003 (`.doc`).
    Doc = 0,
    /// WordprocessingML (`.docx`, `.docm`).
    Docx = 1,
    /// OpenDocument Text (`.odt`).
    Odt = 2,
    /// PDF.
    Pdf = 3,
    /// Binary PowerPoint 97-2003 (`.ppt`, `.pps`, `.pot`).
    Ppt = 4,
    /// PresentationML (`.pptx`, `.pptm`, `.ppsx`, `.ppsm`).
    Pptx = 5,
    /// Rich Text Format (`.rtf`).
    Rtf = 6,
    /// EPUB.
    Epub = 7,
    /// Excel workbooks (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`).
    Excel = 8,
    /// OpenDocument Spreadsheet (`.ods`).
    Ods = 9,
    /// OpenDocument Presentation (`.odp`).
    Odp = 10,
    /// Delimiter-separated text (`.csv`).
    Csv = 11,
}

fn format_to_code(format: Option<Format>) -> AnyDocFormat {
    match format {
        Some(Format::Doc) => AnyDocFormat::Doc,
        Some(Format::Docx) => AnyDocFormat::Docx,
        Some(Format::Odt) => AnyDocFormat::Odt,
        Some(Format::Pdf) => AnyDocFormat::Pdf,
        Some(Format::Ppt) => AnyDocFormat::Ppt,
        Some(Format::Pptx) => AnyDocFormat::Pptx,
        Some(Format::Rtf) => AnyDocFormat::Rtf,
        Some(Format::Epub) => AnyDocFormat::Epub,
        Some(Format::Excel) => AnyDocFormat::Excel,
        Some(Format::Ods) => AnyDocFormat::Ods,
        Some(Format::Odp) => AnyDocFormat::Odp,
        Some(Format::Csv) => AnyDocFormat::Csv,
        None => AnyDocFormat::Unknown,
    }
}

fn code_to_format(code: c_int) -> Option<Format> {
    Some(match code {
        0 => Format::Doc,
        1 => Format::Docx,
        2 => Format::Odt,
        3 => Format::Pdf,
        4 => Format::Ppt,
        5 => Format::Pptx,
        6 => Format::Rtf,
        7 => Format::Epub,
        8 => Format::Excel,
        9 => Format::Ods,
        10 => Format::Odp,
        11 => Format::Csv,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Last-error slot
// ---------------------------------------------------------------------------

thread_local! {
    /// The most recent error message, for retrieval via [`anydoc_last_error`].
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };

    /// The most recent error code, for retrieval via [`anydoc_error_code`].
    static LAST_ERROR_CODE: RefCell<c_int> = const { RefCell::new(0) };
}

fn set_error(err: &ConvertError) {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = err.to_string());
    LAST_ERROR_CODE.with(|slot| *slot.borrow_mut() = error_code(err) as c_int);
}

fn clear_error() {
    LAST_ERROR.with(|slot| slot.borrow_mut().clear());
    LAST_ERROR_CODE.with(|slot| *slot.borrow_mut() = 0);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert an owned `String` into a heap-allocated, NUL-terminated `char*`
/// owned by the caller. Panics if the string contains an interior NUL (which
/// would truncate silently across the boundary).
fn into_raw_string(s: String) -> *mut c_char {
    CString::new(s)
        .expect("C ABI string must not contain interior NUL bytes")
        .into_raw()
}

/// SAFETY: `ptr` must be a valid pointer previously returned by
/// [`into_raw_string`] / an `anydoc_*` function, or NULL.
unsafe fn free_raw_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: caller guarantees a pointer previously returned by us.
        // Re-take ownership and drop it.
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

// SAFETY: `data` must be non-NULL and `len` must be `<=` the allocated size.
unsafe fn slice_from_parts(data: *const u8, len: usize) -> &'static [u8] {
    if data.is_null() {
        &[]
    } else {
        // SAFETY: caller guarantees a valid buffer of `len` bytes.
        unsafe { slice::from_raw_parts(data, len) }
    }
}

// SAFETY: `s` must be a valid NUL-terminated C string, or NULL.
unsafe fn cstr_to_str<'a>(s: *const c_char) -> &'a str {
    if s.is_null() {
        return "";
    }
    // SAFETY: caller guarantees a valid NUL-terminated string.
    unsafe { CStr::from_ptr(s) }.to_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// Exported functions
// ---------------------------------------------------------------------------

/// Convert a document file (by path) to Markdown.
///
/// Returns a heap-allocated UTF-8 string on success (caller must free with
/// [`anydoc_string_free`]), or NULL on error (use [`anydoc_error_code`] and
/// [`anydoc_last_error`] for details).
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 path string, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_to_markdown(path: *const c_char) -> *mut c_char {
    let path = unsafe { cstr_to_str(path) };
    match crate::to_markdown(path) {
        Ok(md) => {
            clear_error();
            into_raw_string(md)
        }
        Err(err) => {
            set_error(&err);
            ptr::null_mut()
        }
    }
}

/// Convert in-memory document bytes to Markdown.
///
/// `format` is an [`AnyDocFormat`] code, or `AnyDocFormat::Unknown` (-1) to
/// auto-detect from the content. Signature-less formats (CSV) must be named
/// explicitly. Returns a heap-allocated UTF-8 string (free with
/// [`anydoc_string_free`]) or NULL on error.
///
/// # Safety
/// `data` must be non-NULL with at least `len` readable bytes, or NULL with
/// `len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_to_markdown_bytes(
    data: *const u8,
    len: usize,
    format: c_int,
) -> *mut c_char {
    let bytes = unsafe { slice_from_parts(data, len) };
    let format = code_to_format(format);
    match crate::to_markdown_bytes(bytes, format) {
        Ok(md) => {
            clear_error();
            into_raw_string(md)
        }
        Err(err) => {
            set_error(&err);
            ptr::null_mut()
        }
    }
}

/// Detect the format from content bytes. Returns an [`AnyDocFormat`] code, or
/// `AnyDocFormat::Unknown` (-1) for signature-less / unrecognized content.
///
/// # Safety
/// `data` must be non-NULL with at least `len` readable bytes, or NULL with
/// `len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_format_from_bytes(data: *const u8, len: usize) -> c_int {
    let bytes = unsafe { slice_from_parts(data, len) };
    format_to_code(Format::from_bytes(bytes)) as c_int
}

/// Detect the format from a bare extension (no leading dot). Returns an
/// [`AnyDocFormat`] code, or `AnyDocFormat::Unknown` (-1).
///
/// # Safety
/// `ext` must be a valid NUL-terminated UTF-8 string, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_format_from_extension(ext: *const c_char) -> c_int {
    let ext = unsafe { cstr_to_str(ext) };
    format_to_code(Format::from_extension(ext)) as c_int
}

/// Detect the format from a path's extension. Returns an [`AnyDocFormat`]
/// code, or `AnyDocFormat::Unknown` (-1) when the path has no recognized
/// extension.
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 path string, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_format_from_path(path: *const c_char) -> c_int {
    let path = unsafe { cstr_to_str(path) };
    format_to_code(Format::from_path(std::path::Path::new(path))) as c_int
}

/// Free a string previously returned by an `anydoc_*` function.
///
/// # Safety
/// `ptr` must be a pointer previously returned by an `anydoc_*` function, or
/// NULL (in which case this is a no-op). Do not call twice on the same pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn anydoc_string_free(ptr: *mut c_char) {
    unsafe { free_raw_string(ptr) }
}

/// The most recent error message. Returns a heap-allocated UTF-8 string the
/// caller must free with [`anydoc_string_free`], or NULL if no error has been
/// recorded.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_last_error() -> *mut c_char {
    LAST_ERROR.with(|slot| {
        let msg = slot.borrow().clone();
        if msg.is_empty() {
            ptr::null_mut()
        } else {
            into_raw_string(msg)
        }
    })
}

/// The most recent error code ([`AnyDocErrorCode`]); `0` when no error has
/// been recorded.
#[unsafe(no_mangle)]
pub extern "C" fn anydoc_error_code() -> c_int {
    LAST_ERROR_CODE.with(|slot| *slot.borrow())
}
