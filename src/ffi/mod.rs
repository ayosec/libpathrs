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

// Import ourselves to make this an example of using libpathrs.
use crate as libpathrs;
use libpathrs::syscalls;
use libpathrs::{Error, Handle, InodeType, OpenFlags, RenameFlags, Resolver, Root};

use std::ffi::{CStr, CString, OsStr};
use std::fs::Permissions;
use std::io::Error as IOError;
use std::os::unix::{
    ffi::OsStrExt,
    fs::PermissionsExt,
    io::{AsRawFd, IntoRawFd, RawFd},
};
use std::path::Path;
use std::{mem, ptr};

use backtrace::Backtrace;
use failure::Error as FailureError;
use libc::{c_char, c_int, c_uint, c_void, dev_t};

trait Leakable {
    /// Leak a structure such that it can be passed through C-FFI.
    fn leak(self) -> &'static mut Self;

    /// Given a structure leaked through Leakable::leak, un-leak it. Callers
    /// must be sure to only ever call this once on a given pointer (otherwise
    /// memory corruption will occur).
    fn unleak(&'static mut self) -> Self;

    /// Shorthand for `std::mem::drop(self.unleak())`.
    fn free(&'static mut self);
}

/// A macro to implement the trivial methods of Leakable -- due to a restriction
/// of the Rust compiler (you cannot have default trait methods that use Self
/// directly, because the size of Self is not known by the trait).
macro_rules! leakable {
    (...) => {
        fn leak(self) -> &'static mut Self {
            Box::leak(Box::new(self))
        }

        fn unleak(&'static mut self) -> Self {
            // Box::from_raw is safe because the C caller guarantees that the
            // pointer we get is the same one we gave them, and it will only ever be
            // called once with the same pointer.
            *unsafe { Box::from_raw(self as *mut Self) }
        }

        fn free(&'static mut self) {
            let _ = self.unleak();
            // drop Self
        }
    };

    (impl Leakable for $type:ty ;) => {
        impl Leakable for $type {
            leakable!(...);
        }
    };

    // Use [] instead of <> to get around a macro_rules! limitation.
    (impl [$($generics:tt),+] Leakable for $type:ty ;) => {
        impl<$($generics),+> Leakable for $type {
            leakable!(...);
        }
    };
}

/// This is only exported to work around a Rust compiler restriction. Consider
/// it an implementation detail and don't make use of it.
// Wrapping struct which we can given C a pointer to. &T isn't an option,
// because DSTs (fat pointers) like dyn T (and thus &dyn T) have no FFI-safe
// representation. So we need to hide it within an FFI-safe pointer (such as a
// trivial struct).
pub struct CPointer<T> {
    inner: T,
    last_error: Option<FailureError>,
}

leakable! {
    impl[T] Leakable for CPointer<T>;
}

impl<T> From<T> for CPointer<T> {
    fn from(inner: T) -> Self {
        CPointer {
            inner: inner,
            last_error: None,
        }
    }
}

/// A handle to the root of a directory tree to resolve within. The only purpose
/// of this "root handle" is to get Handles to inodes within the directory tree.
///
/// At the time of writing, it is considered a *VERY BAD IDEA* to open a Root
/// inside a possibly-attacker-controlled directory tree. While we do have
/// protections that should defend against it (for both drivers), it's far more
/// dangerous than just opening a directory tree which is not inside a
/// potentially-untrusted directory.
pub type CRoot = CPointer<Root>;

/// A handle to a path within a given Root. This handle references an
/// already-resolved path which can be used for only one purpose -- to "re-open"
/// the handle and get an actual fs::File which can be used for ordinary
/// operations.
///
/// It is critical for the safety of users of this library that *at no point* do
/// you use interfaces like libc::openat directly on file descriptors you get
/// from using this library (or extract the RawFd from a fs::File). You must
/// always use operations through a Root.
pub type CHandle = CPointer<Handle>;

// Private trait necessary to work around the "orphan trait" restriction.
trait ErrorWrap {
    fn wrap<F, R>(&mut self, c_err: R, func: F) -> R
    where
        F: FnOnce() -> Result<R, FailureError>;
}

impl ErrorWrap for Option<FailureError> {
    /// Very helpful wrapper to use in "pub extern fn" Rust FFI functions, to
    /// allow for error handling to be done in a much more Rust-like manner. The
    /// idea is that the Rust error is stored in some fixed variable (hopefully
    /// associated with the object being operated on) while the C FFI binding
    /// returns a C-friendly error code.
    ///
    /// ```
    /// # use std::os::raw::c_char;
    /// # fn main() {}
    /// use failure::Error;
    /// use libpathrs::ffi::error;
    ///
    /// #[no_mangle]
    /// pub extern fn func(msg: *const c_char) -> c_int {
    ///     let mut last_error: Option<Error> = None;
    ///     last_error.ffi_wrap(-1, move || {
    ///         if msg.is_null() {
    ///             bail!("null pointer!");
    ///         }
    ///         Ok(42)
    ///     })
    /// }
    /// ```
    fn wrap<F, R>(&mut self, c_err: R, func: F) -> R
    where
        F: FnOnce() -> Result<R, FailureError>,
    {
        // Clear the error before the operation to avoid the "errno problem".
        *self = None;
        func().unwrap_or_else(|err| {
            *self = Some(err);
            c_err
        })
    }
}

/// Represents a Rust Vec<T> in an FFI-safe way. It is absolutely critical that
/// the FFI user does not modify *any* of these fields.
#[repr(C)]
pub struct CVec<T> {
    /// Pointer to the head of the vector.
    pub head: *const T,
    /// Number of elements in the vector (must not be modified).
    pub length: usize,
    /// Capacity of the vector (must not be modified).
    pub __capacity: usize,
}

leakable! {
    impl[T] Leakable for CVec<T>;
}

impl<T> From<Vec<T>> for CVec<T> {
    fn from(vec: Vec<T>) -> Self {
        let head = vec.as_ptr();
        let length = vec.len();
        let capacity = vec.capacity();

        // We now in charge of Vec's memory.
        mem::forget(vec);

        CVec {
            head: head,
            length: length,
            __capacity: capacity,
        }
    }
}

impl<T> Drop for CVec<T> {
    fn drop(&mut self) {
        if self.head.is_null() {
            // Vec::from_raw_parts is safe because the C caller guarantees that the
            // (pointer, length, capacity) tuple is unchanged from when we created
            // the CVec.
            let _ =
                unsafe { Vec::from_raw_parts(self.head as *mut T, self.length, self.__capacity) };
            // Clear the pointer to avoid double-frees.
            self.head = ptr::null_mut();
            // drop the Vec and all its contents
        }
    }
}

/// Represents a single entry in a Rust backtrace in C. This structure is
/// owned by the relevant `pathrs_error_t`.
#[repr(C)]
pub struct CBacktraceEntry {
    /// Instruction pointer at time of backtrace.
    pub ip: *const c_void,

    /// Address of the enclosing symbol at time of backtrace.
    pub symbol_address: *const c_void,

    /// Symbol name for @symbol_address (or NULL if none could be resolved).
    pub symbol_name: *const c_char,

    /// Filename in which the symbol is defined (or NULL if none could be
    /// resolved -- usually due to lack of debugging symbols).
    pub symbol_file: *const c_char,

    /// Line within @symbol_file on which the symbol is defined (will only make
    /// sense if @symbol_file is non-NULL).
    pub symbol_lineno: u32,
}

impl Drop for CBacktraceEntry {
    fn drop(&mut self) {
        if !self.symbol_name.is_null() {
            // CString::from_raw is safe because the C caller guarantees that
            // the pointer we get is the same one we gave them.
            let _ = unsafe { CString::from_raw(self.symbol_name as *mut c_char) };
            // Clear the pointer to avoid double-frees.
            self.symbol_name = ptr::null_mut();
            // drop the CString
        }
        if !self.symbol_file.is_null() {
            // CString::from_raw is safe because the C caller guarantees that
            // the pointer we get is the same one we gave them.
            let _ = unsafe { CString::from_raw(self.symbol_file as *mut c_char) };
            // Clear the pointer to avoid double-frees.
            self.symbol_file = ptr::null_mut();
            // drop the CString
        }
    }
}

/// This is only exported to work around a Rust compiler restriction. Consider
/// it an implementation detail and don't make use of it.
pub type CBacktrace = CVec<CBacktraceEntry>;

impl From<Backtrace> for CBacktrace {
    fn from(mut backtrace: Backtrace) -> Self {
        // Make sure we've resolved as many symbols as possible.
        backtrace.resolve();

        // Construct a Vec<CBacktraceEntry> for leaking.
        backtrace
            .frames()
            .iter()
            .map(|frame| {
                let symbol = frame.symbols().last();

                // XXX: Option::flatten is in Rust 1.40.0 stable.

                let (name, file, lineno) = match symbol {
                    Some(symbol) => {
                        let name = symbol.name().map(|name| {
                            CString::new(name.to_string()).expect(
                                "CString::new(symbol_name) failed in CBacktraceEntry generation",
                            )
                        });

                        let file = symbol.filename().map(|file| {
                            CString::new(file.as_os_str().as_bytes()).expect(
                                "CString::new(symbol_file) failed in CBacktraceEntry generation",
                            )
                        });

                        (name, file, symbol.lineno())
                    }
                    None => (None, None, None),
                };

                CBacktraceEntry {
                    ip: frame.ip(),
                    symbol_address: frame.symbol_address(),
                    symbol_name: name.map(CString::into_raw).unwrap_or(ptr::null_mut())
                        as *const c_char,
                    symbol_file: file.map(CString::into_raw).unwrap_or(ptr::null_mut())
                        as *const c_char,
                    symbol_lineno: lineno.unwrap_or(0),
                }
            })
            .collect::<Vec<_>>()
            .into()
    }
}

/// Attempts to represent a Rust Error type in C. This structure must be freed
/// using `pathrs_free(PATHRS_ERROR)`.
#[repr(C)]
pub struct CError {
    /// Raw errno(3) value of the underlying error (or 0 if the source of the
    /// error was not due to a syscall error).
    // We can't call this field "errno" because glibc defines errno(3) as a
    // macro, causing all sorts of problems if you have a struct with an "errno"
    // field. Best to avoid those headaches.
    pub saved_errno: i32,

    /// Textual description of the error.
    pub description: *const c_char,

    /// Backtrace captured at the error site (or NULL if backtraces have been
    /// disabled at libpathrs build-time or through an environment variable).
    pub backtrace: Option<&'static mut CBacktrace>,
}

leakable! {
    impl Leakable for CError;
}

impl From<&FailureError> for CError {
    /// Construct a new CError struct based on the given error. The description
    /// is pretty-printed in a C-like manner (causes are appended to one another
    /// with separating colons). In addition, if the root-cause of the error is
    /// an IOError then errno is populated with that value.
    fn from(err: &FailureError) -> Self {
        let fail = err.as_fail();
        let desc = err
            .iter_causes()
            .fold(fail.to_string(), |prev, next| format!("{}: {}", prev, next));

        // Create a C-compatible string for CError.description.
        let desc =
            CString::new(desc).expect("CString::new(description) failed in CError generation");

        let errno = match fail.find_root_cause().downcast_ref::<IOError>() {
            Some(err) => err.raw_os_error().unwrap_or(0),
            _ => 0,
        };

        // TODO: Actually have a backtrace created at error-creation time. In
        //       order to do this nicely we will need to switch to snafu, so
        //       this is just used as a PoC.
        let backtrace: CBacktrace = Backtrace::new().into();

        CError {
            saved_errno: errno,
            description: desc.into_raw(),
            backtrace: backtrace.leak().into(),
        }
    }
}

impl Drop for CError {
    fn drop(&mut self) {
        if !self.description.is_null() {
            // CString::from_raw is safe because the C caller guarantees that
            // the pointer we get is the same one we gave them.
            let _ = unsafe { CString::from_raw(self.description as *mut c_char) };
            // Clear the pointer to avoid double-frees.
            self.description = ptr::null_mut();
            // drop the CString
        }

        if let Some(ref mut backtrace) = self.backtrace {
            // The following is an exceptionally dirty hack to deal with the
            // fact that we cannot move a &'static mut in this context. However,
            // this is all okay because the &'static mut is being used as a
            // pointer to a leaked CBacktrace.
            let backtrace = *backtrace as *mut CBacktrace;
            // Remove self.backtrace reference before we do free it. We don't
            // want something to dereference it.
            self.backtrace = None;
            // An finally, free the backtrace.
            unsafe { &mut *backtrace }.free();
        }
    }
}

/// Copy the currently-stored infomation into the provided buffer.
///
/// If there was a stored error, a positive value is returned. If there was no
/// stored error, the contents of buffer are undefined and 0 is returned. If an
/// internal error occurs during processing, -1 is returned.
#[no_mangle]
pub extern "C" fn pathrs_error(
    ptr_type: CPointerType,
    ptr: *mut c_void,
) -> Option<&'static mut CError> {
    if ptr.is_null() {
        return None;
    }

    // Both of these casts and dereferences are safe because the C caller has
    // assured us that the type passed is correct.
    let last_error = match ptr_type {
        CPointerType::PATHRS_ERROR => return None,
        CPointerType::PATHRS_ROOT => {
            let root = unsafe { &mut *(ptr as *mut CRoot) };
            &mut root.last_error
        }
        CPointerType::PATHRS_HANDLE => {
            let handle = unsafe { &mut *(ptr as *mut CHandle) };
            &mut handle.last_error
        }
    };

    last_error.as_ref().map(CError::from).map(Leakable::leak)
}

fn parse_path<'a>(path: *const c_char) -> Result<&'a Path, FailureError> {
    if path.is_null() {
        Err(Error::InvalidArgument("path", "cannot be NULL"))?;
    }
    let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    Ok(OsStr::from_bytes(bytes).as_ref())
}

/// Open a root handle.
///
/// The default resolver is automatically chosen based on the running kernel.
/// You can switch the resolver used with pathrs_set_resolver() -- though this
/// is not strictly recommended unless you have a good reason to do it.
///
/// The provided path must be an existing directory. If using the emulated
/// driver, it also must be the fully-expanded path to a real directory (with no
/// symlink components) because the given path is used to double-check that the
/// open operation was not affected by an attacker.
#[no_mangle]
pub extern "C" fn pathrs_open(path: *const c_char) -> Option<&'static mut CRoot> {
    // Leak the box so we can return the pointer to the caller.
    // TODO: Return the error back to the caller somehow.
    let mut _error: Option<FailureError> = None;
    _error.wrap(None, move || {
        Root::open(parse_path(path)?)
            .map(CRoot::from)
            .map(Leakable::leak)
            .map(Option::from)
    })
}

/// The backend used for path resolution within a pathrs_root_t to get a
/// pathrs_handle_t.
#[repr(C)]
#[allow(non_camel_case_types, dead_code)]
pub enum CResolver {
    /// Use the native openat2(2) backend (requires kernel support).
    PATHRS_KERNEL_RESOLVER,
    /// Use the userspace "emulated" backend.
    PATHRS_EMULATED_RESOLVER,
}

impl Into<Resolver> for CResolver {
    fn into(self) -> Resolver {
        match self {
            CResolver::PATHRS_KERNEL_RESOLVER => Resolver::Kernel,
            CResolver::PATHRS_EMULATED_RESOLVER => Resolver::Emulated,
        }
    }
}

/// Switch the resolver for the given root handle.
#[no_mangle]
pub extern "C" fn pathrs_set_resolver(root: &mut CRoot, resolver: CResolver) {
    root.inner.with_resolver(resolver.into());
}

/// The type of object being passed to "object agnostic" libpathrs functions.
#[repr(C)]
#[allow(non_camel_case_types, dead_code)]
pub enum CPointerType {
    /// `pathrs_error_t`
    PATHRS_ERROR,
    /// `pathrs_root_t`
    PATHRS_ROOT,
    /// `pathrs_handle_t`
    PATHRS_HANDLE,
}

/// Free a libpathrs object. It is critical that users pass the correct @type --
/// not doing so will certainly trigger memory unsafety bugs.
#[no_mangle]
pub extern "C" fn pathrs_free(ptr_type: CPointerType, ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }

    // Both of these casts and dereferences are safe because the C caller has
    // assured us that the type passed is correct.
    match ptr_type {
        CPointerType::PATHRS_ERROR => unsafe { &mut *(ptr as *mut CError) }.free(),
        CPointerType::PATHRS_ROOT => unsafe { &mut *(ptr as *mut CRoot) }.free(),
        CPointerType::PATHRS_HANDLE => unsafe { &mut *(ptr as *mut CHandle) }.free(),
    }
}

/// "Upgrade" the handle to a usable fd, suitable for reading and writing. This
/// does not consume the original handle (allowing for it to be used many
/// times).
///
/// It should be noted that the use of O_CREAT *is not* supported (and will
/// result in an error). Handles only refer to *existing* files. Instead you
/// need to use inroot_creat().
///
/// In addition, O_NOCTTY is automatically set when opening the path. If you
/// want to use the path as a controlling terminal, you will have to do
/// ioctl(fd, TIOCSCTTY, 0) yourself.
#[no_mangle]
pub extern "C" fn pathrs_reopen(handle: &mut CHandle, flags: c_int) -> RawFd {
    let flags = OpenFlags(flags);
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &handle.inner;

    handle.last_error.wrap(-1, || {
        let file = inner.reopen(flags)?;
        // Rust sets O_CLOEXEC by default, without an opt-out. We need to
        // disable it if we weren't asked to do O_CLOEXEC.
        if flags.0 & libc::O_CLOEXEC == 0 {
            syscalls::fcntl_unset_cloexec(file.as_raw_fd())?;
        }
        Ok(file.into_raw_fd())
    })
}

/// Within the given root's tree, resolve the given path (with all symlinks
/// being scoped to the root) and return a handle to that path. The path *must
/// already exist*, otherwise an error will occur.
#[no_mangle]
pub extern "C" fn pathrs_inroot_resolve(
    root: &mut CRoot,
    path: *const c_char,
) -> Option<&'static mut CHandle> {
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(None, move || {
        inner
            .resolve(parse_path(path)?)
            .map(CHandle::from)
            .map(Leakable::leak)
            .map(Option::from)
    })
}

/// Within the given root's tree, perform the rename (with all symlinks being
/// scoped to the root). The flags argument is identical to the renameat2(2)
/// flags that are supported on the system.
#[no_mangle]
pub extern "C" fn pathrs_inroot_rename(
    root: &mut CRoot,
    src: *const c_char,
    dst: *const c_char,
    flags: c_int,
) -> c_int {
    let flags = RenameFlags(flags);
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(-1, move || {
        inner
            .rename(parse_path(src)?, parse_path(dst)?, flags)
            .and(Ok(0))
    })
}

// Within the root, create an inode at the path with the given mode. If the
// path already exists, an error is returned (effectively acting as though
// O_EXCL is always set). Each inroot_* corresponds to the matching syscall.

// TODO: Replace all the inroot_* stuff with macros. It's quite repetitive.

#[no_mangle]
pub extern "C" fn pathrs_inroot_creat(
    root: &mut CRoot,
    path: *const c_char,
    mode: c_uint,
) -> Option<&'static mut CHandle> {
    let mode = mode & !libc::S_IFMT;
    let perm = Permissions::from_mode(mode);
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(None, move || {
        inner
            .create_file(parse_path(path)?, &perm)
            .map(CHandle::from)
            .map(Leakable::leak)
            .map(Option::from)
    })
}

#[no_mangle]
pub extern "C" fn pathrs_inroot_mkdir(
    root: &mut CRoot,
    path: *const c_char,
    mode: c_uint,
) -> c_int {
    let mode = mode & !libc::S_IFMT;

    pathrs_inroot_mknod(root, path, libc::S_IFDIR | mode, 0)
}

#[no_mangle]
pub extern "C" fn pathrs_inroot_mknod(
    root: &mut CRoot,
    path: *const c_char,
    mode: c_uint,
    dev: dev_t,
) -> c_int {
    let fmt = mode & libc::S_IFMT;
    let perms = Permissions::from_mode(mode ^ fmt);
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(-1, move || {
        let path = parse_path(path)?;
        let inode_type = match fmt {
            libc::S_IFREG => InodeType::File(&perms),
            libc::S_IFDIR => InodeType::Directory(&perms),
            libc::S_IFBLK => InodeType::BlockDevice(&perms, dev),
            libc::S_IFCHR => InodeType::CharacterDevice(&perms, dev),
            libc::S_IFIFO => InodeType::Fifo(&perms),
            libc::S_IFSOCK => Err(Error::NotImplemented("mknod(S_IFSOCK)"))?,
            _ => Err(Error::InvalidArgument("mode", "invalid S_IFMT mask"))?,
        };
        inner.create(path, &inode_type).and(Ok(0))
    })
}

#[no_mangle]
pub extern "C" fn pathrs_inroot_symlink(
    root: &mut CRoot,
    path: *const c_char,
    target: *const c_char,
) -> c_int {
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(-1, move || {
        let path = parse_path(path)?;
        let target = parse_path(target)?;

        inner.create(path, &InodeType::Symlink(target)).and(Ok(0))
    })
}

#[no_mangle]
pub extern "C" fn pathrs_inroot_hardlink(
    root: &mut CRoot,
    path: *const c_char,
    target: *const c_char,
) -> c_int {
    // Workaround for https://github.com/rust-lang/rust/issues/53488.
    let inner = &root.inner;

    root.last_error.wrap(-1, move || {
        let path = parse_path(path)?;
        let target = parse_path(target)?;

        inner.create(path, &InodeType::Hardlink(target)).and(Ok(0))
    })
}
