#ifndef ANYDOC_CAPI_H
#define ANYDOC_CAPI_H

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Stable error codes returned by the C ABI. `0` means success.
 *
 * These mirror the values of [`ConvertError::code`]; keep them in sync.
 */
typedef enum AnyDocErrorCode {
  /**
   * Success.
   */
  AnyDocErrorCode_Ok = 0,
  /**
   * The format is unknown or cannot be converted at all.
   */
  AnyDocErrorCode_Unsupported = 1,
  /**
   * The document is structurally unusable.
   */
  AnyDocErrorCode_Malformed = 2,
  /**
   * The document is encrypted or password-protected.
   */
  AnyDocErrorCode_Encrypted = 3,
  /**
   * A fixed safety/resource limit was exceeded.
   */
  AnyDocErrorCode_ResourceLimit = 4,
  /**
   * A required part or stream is missing.
   */
  AnyDocErrorCode_MissingPart = 5,
  /**
   * The input could not be read.
   */
  AnyDocErrorCode_Io = 6,
} AnyDocErrorCode;

/**
 * Stable format codes. `-1` means "unrecognized".
 *
 * These mirror [`Format`] (see `src/lib.rs`).
 */
typedef enum AnyDocFormat {
  /**
   * Unknown / unrecognized.
   */
  AnyDocFormat_Unknown = -1,
  /**
   * Binary Word 97-2003 (`.doc`).
   */
  AnyDocFormat_Doc = 0,
  /**
   * WordprocessingML (`.docx`, `.docm`).
   */
  AnyDocFormat_Docx = 1,
  /**
   * OpenDocument Text (`.odt`).
   */
  AnyDocFormat_Odt = 2,
  /**
   * PDF.
   */
  AnyDocFormat_Pdf = 3,
  /**
   * Binary PowerPoint 97-2003 (`.ppt`, `.pps`, `.pot`).
   */
  AnyDocFormat_Ppt = 4,
  /**
   * PresentationML (`.pptx`, `.pptm`, `.ppsx`, `.ppsm`).
   */
  AnyDocFormat_Pptx = 5,
  /**
   * Rich Text Format (`.rtf`).
   */
  AnyDocFormat_Rtf = 6,
  /**
   * EPUB.
   */
  AnyDocFormat_Epub = 7,
  /**
   * Excel workbooks (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`).
   */
  AnyDocFormat_Excel = 8,
  /**
   * OpenDocument Spreadsheet (`.ods`).
   */
  AnyDocFormat_Ods = 9,
  /**
   * OpenDocument Presentation (`.odp`).
   */
  AnyDocFormat_Odp = 10,
  /**
   * Delimiter-separated text (`.csv`).
   */
  AnyDocFormat_Csv = 11,
} AnyDocFormat;

/**
 * Convert a document file (by path) to Markdown.
 *
 * Returns a heap-allocated UTF-8 string on success (caller must free with
 * [`anydoc_string_free`]), or NULL on error (use [`anydoc_error_code`] and
 * [`anydoc_last_error`] for details).
 *
 * # Safety
 * `path` must be a valid NUL-terminated UTF-8 path string, or NULL.
 */
char *anydoc_to_markdown(const char *path);

/**
 * Convert in-memory document bytes to Markdown.
 *
 * `format` is an [`AnyDocFormat`] code, or `AnyDocFormat::Unknown` (-1) to
 * auto-detect from the content. Signature-less formats (CSV) must be named
 * explicitly. Returns a heap-allocated UTF-8 string (free with
 * [`anydoc_string_free`]) or NULL on error.
 *
 * # Safety
 * `data` must be non-NULL with at least `len` readable bytes, or NULL with
 * `len == 0`.
 */
char *anydoc_to_markdown_bytes(const uint8_t *data, size_t len, int format);

/**
 * Detect the format from content bytes. Returns an [`AnyDocFormat`] code, or
 * `AnyDocFormat::Unknown` (-1) for signature-less / unrecognized content.
 *
 * # Safety
 * `data` must be non-NULL with at least `len` readable bytes, or NULL with
 * `len == 0`.
 */
int anydoc_format_from_bytes(const uint8_t *data, size_t len);

/**
 * Detect the format from a bare extension (no leading dot). Returns an
 * [`AnyDocFormat`] code, or `AnyDocFormat::Unknown` (-1).
 *
 * # Safety
 * `ext` must be a valid NUL-terminated UTF-8 string, or NULL.
 */
int anydoc_format_from_extension(const char *ext);

/**
 * Detect the format from a path's extension. Returns an [`AnyDocFormat`]
 * code, or `AnyDocFormat::Unknown` (-1) when the path has no recognized
 * extension.
 *
 * # Safety
 * `path` must be a valid NUL-terminated UTF-8 path string, or NULL.
 */
int anydoc_format_from_path(const char *path);

/**
 * Free a string previously returned by an `anydoc_*` function.
 *
 * # Safety
 * `ptr` must be a pointer previously returned by an `anydoc_*` function, or
 * NULL (in which case this is a no-op). Do not call twice on the same pointer.
 */
void anydoc_string_free(char *ptr);

/**
 * The most recent error message. Returns a heap-allocated UTF-8 string the
 * caller must free with [`anydoc_string_free`], or NULL if no error has been
 * recorded.
 */
char *anydoc_last_error(void);

/**
 * The most recent error code ([`AnyDocErrorCode`]); `0` when no error has
 * been recorded.
 */
int anydoc_error_code(void);

#endif  /* ANYDOC_CAPI_H */
