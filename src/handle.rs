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

use crate::utils::RawFdExt;

use core::convert::TryFrom;
use std::fs::File;
use std::ops::Deref;

use failure::Error as FailureError;
use libc::c_int;

/// A handle to an existing inode within a [`Root`].
///
/// This handle references an already-resolved path which can be used for the
/// purpose of "re-opening" the handle and get an actual [`File`] which can be
/// used for ordinary operations.
///
/// # Safety
///
/// It is critical for the safety of this library that **at no point** do you
/// use interfaces like [`libc::openat`] directly on any [`RawFd`]s you might
/// extract from the [`File`] you get from this [`Handle`]. **You must always do
/// operations through a valid [`Root`].**
///
/// [`Root`]: struct.Root.html
/// [`Handle`]: trait.Handle.html
/// [`File`]: https://doc.rust-lang.org/std/fs/struct.File.html
/// [`RawFd`]: https://doc.rust-lang.org/std/os/unix/io/type.RawFd.html
/// [`libc::openat`]: https://docs.rs/libc/latest/libc/fn.openat.html
pub struct Handle(File);

// Only used internally by libpathrs.
#[doc(hidden)]
impl TryFrom<File> for Handle {
    type Error = FailureError;
    fn try_from(file: File) -> Result<Self, Self::Error> {
        // TODO: Check if the file is valid.
        Ok(Handle(file))
    }
}

// Only used internally by libpathrs.
#[doc(hidden)]
impl Deref for Handle {
    type Target = File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper for the underlying `libc`'s `O_*` flags. The flag values and their
/// meaning is identical to the description in the `open(2)` man page.
///
/// # Caveats
///
/// For historical reasons, the first three bits of `open(2)`'s flags are for
/// the access mode and are actually treated as a 2-bit number. So, it is
/// incorrect to attempt to do any checks on the access mode without masking it
/// correctly. So some helpers were added to make usage more ergonomic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenFlags(pub c_int);

impl OpenFlags {
    /// Grab the access mode bits from the flags.
    #[inline]
    pub fn access_mode(&self) -> c_int {
        self.0 & libc::O_ACCMODE
    }

    /// Does the access mode imply read access?
    #[inline]
    pub fn wants_read(&self) -> bool {
        let acc = self.access_mode();
        acc == libc::O_RDONLY || acc == libc::O_RDWR
    }

    /// Does the access mode imply write access? Note that there are several
    /// other bits (such as O_TRUNC) which imply write access but are not part
    /// of the access mode, and thus a `false` value from `.wants_write()` does
    /// not guarantee that the kernel will not do a `MAY_WRITE` check.
    #[inline]
    pub fn wants_write(&self) -> bool {
        let acc = self.access_mode();
        acc == libc::O_WRONLY || acc == libc::O_RDWR
    }
}

impl Handle {
    /// "Upgrade" the handle to a usable [`File`] handle suitable for reading
    /// and writing. This does not consume the original handle (allowing for it
    /// to be used many times).
    ///
    /// [`File`]: https://doc.rust-lang.org/std/fs/struct.File.html
    /// [`Root::create`]: struct.Root.html#method.create
    pub fn reopen(&self, flags: OpenFlags) -> Result<File, FailureError> {
        self.0.reopen(flags)
    }

    // TODO: All the different stat* interfaces?

    // TODO: bind(). This might be safe to do (set the socket path to
    //       /proc/self/fd/...) but I'm a bit sad it'd be separate from
    //       Handle::reopen().
}
