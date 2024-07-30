/*
 * libpathrs: safe path resolution on Linux
 * Copyright (C) 2019-2024 Aleksa Sarai <cyphar@cyphar.com>
 * Copyright (C) 2019-2024 SUSE LLC
 *
 * This program is free software: you can redistribute it and/or modify it under
 * the terms of the GNU Lesser General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option) any
 * later version.
 *
 * This program is distributed in the hope that it will be useful, but WITHOUT ANY
 * WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
 * PARTICULAR PURPOSE. See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public License along
 * with this program. If not, see <https://www.gnu.org/licenses/>.
 */

#ifdef __CBINDGEN_ALIGNED
#undef __CBINDGEN_ALIGNED
#endif
#define __CBINDGEN_ALIGNED(n) __attribute__((aligned(n)))


#ifndef LIBPATHRS_H
#define LIBPATHRS_H

/*
 * WARNING: This file was auto-generated by rust-cbindgen. Don't modify it.
 *          Instead, re-generate it with:
 *            % cbindgen -c cbindgen.toml -o include/pathrs.h
 */


#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/types.h>

/**
 * Indicate what base directory should be used when doing operations with
 * pathrs_proc_*. This is necessary because /proc/thread-self is not present on
 * pre-3.17 kernels and so it may be necessary to emulate /proc/thread-self
 * access on those older kernels.
 *
 * NOTE: Currently, operating on /proc/... directly is not supported.
 *
 * [`ProcfsHandle`]: struct.ProcfsHandle.html
 */
typedef enum {
    /**
     * Use /proc/self. For most programs, this is the standard choice.
     */
    PATHRS_PROC_SELF = 152919583,
    /**
     * Use /proc/thread-self. In multi-threaded programs where one thread has a
     * different CLONE_FS, it is possible for /proc/self to point the wrong
     * thread and so /proc/thread-self may be necessary.
     *
     * NOTE: Using /proc/thread-self may require care if used from langauges
     * where your code can change threads without warning and old threads can
     * be killed (such as Go -- where you want to use runtime.LockOSThread).
     */
    PATHRS_PROC_THREAD_SELF = 1051549215,
} pathrs_proc_base_t;

/**
 * Attempts to represent a Rust Error type in C. This structure must be freed
 * using pathrs_errorinfo_free().
 */
typedef struct __CBINDGEN_ALIGNED(8) {
    /**
     * Raw errno(3) value of the underlying error (or 0 if the source of the
     * error was not due to a syscall error).
     */
    uint64_t saved_errno;
    /**
     * Textual description of the error.
     */
    const char *description;
} pathrs_error_t;

/**
 * Open a root handle.
 *
 * The provided path must be an existing directory.
 *
 * Note that root handles are not special -- this function is effectively
 * equivalent to
 *
 * ```c
 * fd = open(path, O_PATH|O_DIRECTORY);
 * ```
 *
 * # Return Value
 *
 * On success, this function returns a file descriptor that can be used as a
 * root handle in subsequent pathrs_* operations.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_root_open(const char *path);

/**
 * "Upgrade" an O_PATH file descriptor to a usable fd, suitable for reading and
 * writing. This does not consume the original file descriptor. (This can be
 * used with non-O_PATH file descriptors as well.)
 *
 * It should be noted that the use of O_CREAT *is not* supported (and will
 * result in an error). Handles only refer to *existing* files. Instead you
 * need to use pathrs_creat().
 *
 * In addition, O_NOCTTY is automatically set when opening the path. If you
 * want to use the path as a controlling terminal, you will have to do
 * ioctl(fd, TIOCSCTTY, 0) yourself.
 *
 * # Return Value
 *
 * On success, this function returns a file descriptor.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_reopen(int fd, int flags);

/**
 * Resolve the given path within the rootfs referenced by root_fd. The path
 * *must already exist*, otherwise an error will occur.
 *
 * All symlinks (including trailing symlinks) are followed, but they are
 * resolved within the rootfs. If you wish to open a handle to the symlink
 * itself, use pathrs_resolve_nofollow().
 *
 * # Return Value
 *
 * On success, this function returns an O_PATH file descriptor referencing the
 * resolved path.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_resolve(int root_fd, const char *path);

/**
 * pathrs_resolve_nofollow() is effectively an O_NOFOLLOW version of
 * pathrs_resolve(). Their behaviour is identical, except that *trailing*
 * symlinks will not be followed. If the final component is a trailing symlink,
 * an O_PATH|O_NOFOLLOW handle to the symlink itself is returned.
 *
 * # Return Value
 *
 * On success, this function returns an O_PATH file descriptor referencing the
 * resolved path.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_resolve_nofollow(int root_fd, const char *path);

/**
 * Get the target of a symlink within the rootfs referenced by root_fd.
 *
 * NOTE: The returned path is not modified to be "safe" outside of the
 * root. You should not use this path for doing further path lookups -- use
 * pathrs_resolve() instead.
 *
 * This method is just shorthand for:
 *
 * ```c
 * int linkfd = pathrs_resolve_nofollow(rootfd, path);
 * if (linkfd < 0) {
 *     liberr = fd; // for use with pathrs_errorinfo()
 *     goto err;
 * }
 * copied = readlinkat(linkfd, "", linkbuf, linkbuf_size);
 * close(linkfd);
 * ```
 *
 * # Return Value
 *
 * On success, this function copies the symlink contents to `linkbuf` (up to
 * `linkbuf_size` bytes) and returns the full size of the symlink path buffer.
 * This function will not copy the trailing NUL byte, and the return size does
 * not include the NUL byte. A `NULL` `linkbuf` or invalid `linkbuf_size` are
 * treated as zero-size buffers.
 *
 * NOTE: Unlike readlinkat(2), in the case where linkbuf is too small to
 * contain the symlink contents, pathrs_readlink() will return *the number of
 * bytes it would have copied if the buffer was large enough*. This matches the
 * behaviour of pathrs_proc_readlink().
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_readlink(int root_fd,
                    const char *path,
                    char *linkbuf,
                    size_t linkbuf_size);

/**
 * Rename a path within the rootfs referenced by root_fd. The flags argument is
 * identical to the renameat2(2) flags that are supported on the system.
 *
 * # Return Value
 *
 * On success, this function returns 0.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_rename(int root_fd,
                  const char *src,
                  const char *dst,
                  uint32_t flags);

/**
 * Create a new regular file within the rootfs referenced by root_fd. This is
 * effectively an O_CREAT operation, and so (unlike pathrs_resolve()), this
 * function can be used on non-existent paths.
 *
 * If you want to ensure the creation is a new file, use O_EXCL.
 *
 * If you want to create a file without opening a handle to it, you can do
 * pathrs_mknod(root_fd, path, S_IFREG|mode, 0) instead.
 *
 * As with pathrs_reopen(), O_NOCTTY is automatically set when opening the
 * path. If you want to use the path as a controlling terminal, you will have
 * to do ioctl(fd, TIOCSCTTY, 0) yourself.
 *
 * NOTE: Unlike O_CREAT, pathrs_creat() will return an error if the final
 * component is a dangling symlink. O_CREAT will create such files, and while
 * openat2 does support this it would be difficult to implement this in the
 * emulated resolver.
 *
 * # Return Value
 *
 * On success, this function returns a file descriptor to the requested file.
 * The open flags are based on the provided flags.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_creat(int root_fd, const char *path, int flags, unsigned int mode);

/**
 * Create a new directory within the rootfs referenced by root_fd.
 *
 * This is shorthand for pathrs_mknod(root_fd, path, S_IFDIR|mode, 0).
 *
 * # Return Value
 *
 * On success, this function returns 0.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_mkdir(int root_fd, const char *path, unsigned int mode);

/**
 * Create a inode within the rootfs referenced by root_fd. The type of inode to
 * be created is configured using the S_IFMT bits in mode (a-la mknod(2)).
 *
 * # Return Value
 *
 * On success, this function returns 0.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_mknod(int root_fd, const char *path, unsigned int mode, dev_t dev);

/**
 * Create a symlink within the rootfs referenced by root_fd. Note that the
 * symlink target string is not modified when creating the symlink.
 *
 * # Return Value
 *
 * On success, this function returns 0.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_symlink(int root_fd, const char *path, const char *target);

/**
 * Create a hardlink within the rootfs referenced by root_fd. Both the hardlink
 * path and target are resolved within the rootfs.
 *
 * # Return Value
 *
 * On success, this function returns 0.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_hardlink(int root_fd, const char *path, const char *target);

/**
 * Safely open a path inside a `/proc` handle.
 *
 * Any bind-mounts or other over-mounts will (depending on what kernel features
 * are available) be detected and an error will be returned. Non-trailing
 * symlinks are followed but care is taken to ensure the symlinks are
 * legitimate.
 *
 * Unless you intend to open a magic-link, `O_NOFOLLOW` should be set in flags.
 * Lookups with `O_NOFOLLOW` are guaranteed to never be tricked by bind-mounts
 * (on new enough Linux kernels).
 *
 * If you wish to resolve a magic-link, you need to unset `O_NOFOLLOW`.
 * Unfortunately (if libpathrs is using the regular host `/proc` mount), this
 * lookup mode cannot protect you against an attacker that can modify the mount
 * table during this operation.
 *
 * NOTE: Instead of using paths like `/proc/thread-self/fd`, `base` is used to
 * indicate what "base path" inside procfs is used. For example, to re-open a
 * file descriptor:
 *
 * ```c
 * fd = pathrs_proc_open(PATHRS_PROC_THREAD_SELF, "fd/101", O_RDWR);
 * if (fd < 0) {
 *     liberr = fd; // for use with pathrs_errorinfo()
 *     goto err;
 * }
 * ```
 *
 * # Return Value
 *
 * On success, this function returns a file descriptor.
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_proc_open(pathrs_proc_base_t base, const char *path, int flags);

/**
 * Safely read the contents of a symlink inside `/proc`.
 *
 * As with `pathrs_proc_open`, any bind-mounts or other over-mounts will
 * (depending on what kernel features are available) be detected and an error
 * will be returned. Non-trailing symlinks are followed but care is taken to
 * ensure the symlinks are legitimate.
 *
 * This function is effectively shorthand for
 *
 * ```c
 * fd = pathrs_proc_open(base, path, O_PATH|O_NOFOLLOW);
 * if (fd < 0) {
 *     liberr = fd; // for use with pathrs_errorinfo()
 *     goto err;
 * }
 * copied = readlinkat(fd, "", linkbuf, linkbuf_size);
 * close(fd);
 * ```
 *
 * # Return Value
 *
 * On success, this function copies the symlink contents to `linkbuf` (up to
 * `linkbuf_size` bytes) and returns the full size of the symlink path buffer.
 * This function will not copy the trailing NUL byte, and the return size does
 * not include the NUL byte. A `NULL` `linkbuf` or invalid `linkbuf_size` are
 * treated as zero-size buffers.
 *
 * NOTE: Unlike readlinkat(2), in the case where linkbuf is too small to
 * contain the symlink contents, pathrs_proc_readlink() will return *the number
 * of bytes it would have copied if the buffer was large enough*. This matches
 * the behaviour of pathrs_readlink().
 *
 * If an error occurs, this function will return a negative error code. To
 * retrieve information about the error (such as a string describing the error,
 * the system errno(7) value associated with the error, etc), use
 * pathrs_errorinfo().
 */
int pathrs_proc_readlink(pathrs_proc_base_t base,
                         const char *path,
                         char *linkbuf,
                         size_t linkbuf_size);

/**
 * Retrieve error information about an error id returned by a pathrs operation.
 *
 * Whenever an error occurs with libpathrs, a negative number describing that
 * error (the error id) is returned. pathrs_errorinfo() is used to retrieve
 * that information:
 *
 * ```c
 * fd = pathrs_resolve(root, "/foo/bar");
 * if (fd < 0) {
 *     // fd is an error id
 *     pathrs_error_t *error = pathrs_errorinfo(fd);
 *     // ... print the error information ...
 *     pathrs_errorinfo_free(error);
 * }
 * ```
 *
 * Once pathrs_errorinfo() is called for a particular error id, that error id
 * is no longer valid and should not be used for subsequent pathrs_errorinfo()
 * calls.
 *
 * Error ids are only unique from one another until pathrs_errorinfo() is
 * called, at which point the id can be re-used for subsequent errors. The
 * precise format of error ids is completely opaque and they should never be
 * compared directly or used for anything other than with pathrs_errorinfo().
 *
 * Error ids are not thread-specific and thus pathrs_errorinfo() can be called
 * on a different thread to the thread where the operation failed (this is of
 * particular note to green-thread language bindings like Go, where this is
 * important).
 *
 * # Return Value
 *
 * If there was a saved error with the provided id, a pathrs_error_t is
 * returned describing the error. Use pathrs_errorinfo_free() to free the
 * associated memory once you are done with the error.
 */
pathrs_error_t *pathrs_errorinfo(int err_id);

/**
 * Free the pathrs_error_t object returned by pathrs_errorinfo().
 */
void pathrs_errorinfo_free(pathrs_error_t *ptr);

#endif /* LIBPATHRS_H */

#ifdef __CBINDGEN_ALIGNED
#undef __CBINDGEN_ALIGNED
#endif
