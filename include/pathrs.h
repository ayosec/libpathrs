/*
 * libpathrs: safe path resolution on Linux
 * Copyright (C) 2019 Aleksa Sarai <cyphar@cyphar.com>
 * Copyright (C) 2019 SUSE LLC
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
 * The backend used for path resolution within a pathrs_root_t to get a
 * pathrs_handle_t.
 */
typedef enum {
    /**
     * Use the native openat2(2) backend (requires kernel support).
     */
    PATHRS_KERNEL_RESOLVER,
    /**
     * Use the userspace "emulated" backend.
     */
    PATHRS_EMULATED_RESOLVER,
} pathrs_resolver_t;

/**
 * This is only exported to work around a Rust compiler restriction. Consider
 * it an implementation detail and don't make use of it.
 */
typedef struct __pathrs_handle_t __pathrs_handle_t;

/**
 * This is only exported to work around a Rust compiler restriction. Consider
 * it an implementation detail and don't make use of it.
 */
typedef struct __pathrs_root_t __pathrs_root_t;

/**
 * An error description and (optionally) the underlying errno value that
 * triggered it (if there is no underlying errno, it is 0).
 */
typedef struct {
    int32_t errno;
    unsigned char description[1024];
} pathrs_error_t;

/**
 * A handle to a path within a given Root. This handle references an
 * already-resolved path which can be used for only one purpose -- to "re-open"
 * the handle and get an actual fs::File which can be used for ordinary
 * operations.
 * It is critical for the safety of users of this library that *at no point* do
 * you use interfaces like libc::openat directly on file descriptors you get
 * from using this library (or extract the RawFd from a fs::File). You must
 * always use operations through a Root.
 */
typedef __pathrs_handle_t pathrs_handle_t;

/**
 * A handle to the root of a directory tree to resolve within. The only purpose
 * of this "root handle" is to get Handles to inodes within the directory tree.
 * At the time of writing, it is considered a *VERY BAD IDEA* to open a Root
 * inside a possibly-attacker-controlled directory tree. While we do have
 * protections that should defend against it (for both drivers), it's far more
 * dangerous than just opening a directory tree which is not inside a
 * potentially-untrusted directory.
 */
typedef __pathrs_root_t pathrs_root_t;

/**
 * Copy the currently-stored infomation into the provided buffer.
 * If there was a stored error, a positive value is returned. If there was no
 * stored error, the contents of buffer are undefined and 0 is returned. If an
 * internal error occurs during processing, -1 is returned.
 */
int pathrs_error(pathrs_error_t *buffer);

/**
 * Free a handle.
 */
void pathrs_hfree(pathrs_handle_t *handle);

pathrs_handle_t *pathrs_inroot_creat(const pathrs_root_t *root,
                                     const char *path,
                                     unsigned int mode);

int pathrs_inroot_hardlink(const pathrs_root_t *root,
                           const char *path,
                           const char *target);

int pathrs_inroot_mkdir(const pathrs_root_t *root,
                        const char *path,
                        unsigned int mode);

int pathrs_inroot_mknod(const pathrs_root_t *root,
                        const char *path,
                        unsigned int mode,
                        dev_t dev);

/**
 * Within the given root's tree, perform the rename (with all symlinks being
 * scoped to the root). The flags argument is identical to the renameat2(2)
 * flags that are supported on the system.
 */
int pathrs_inroot_rename(const pathrs_root_t *root,
                         const char *src,
                         const char *dst,
                         int flags);

/**
 * Within the given root's tree, resolve the given path (with all symlinks
 * being scoped to the root) and return a handle to that path. The path *must
 * already exist*, otherwise an error will occur.
 */
pathrs_handle_t *pathrs_inroot_resolve(const pathrs_root_t *root,
                                       const char *path);

int pathrs_inroot_symlink(const pathrs_root_t *root,
                          const char *path,
                          const char *target);

/**
 * Open a root handle.
 * The default resolver is automatically chosen based on the running kernel.
 * You can switch the resolver used with pathrs_set_resolver() -- though this
 * is not strictly recommended unless you have a good reason to do it.
 * The provided path must be an existing directory. If using the emulated
 * driver, it also must be the fully-expanded path to a real directory (with no
 * symlink components) because the given path is used to double-check that the
 * open operation was not affected by an attacker.
 */
pathrs_root_t *pathrs_open(const char *path);

/**
 * "Upgrade" the handle to a usable fd, suitable for reading and writing. This
 * does not consume the original handle (allowing for it to be used many
 * times).
 * It should be noted that the use of O_CREAT *is not* supported (and will
 * result in an error). Handles only refer to *existing* files. Instead you
 * need to use inroot_creat().
 * In addition, O_NOCTTY is automatically set when opening the path. If you
 * want to use the path as a controlling terminal, you will have to do
 * ioctl(fd, TIOCSCTTY, 0) yourself.
 */
int pathrs_reopen(const pathrs_handle_t *handle, int flags);

/**
 * Free a root handle.
 */
void pathrs_rfree(pathrs_root_t *root);

/**
 * Switch the resolver for the given root handle.
 */
void pathrs_set_resolver(pathrs_root_t *root, pathrs_resolver_t resolver);

#endif /* LIBPATHRS_H */
