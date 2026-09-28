//! What the stick is formatted as (issue #184). A player that cannot mount a
//! stick shows no library at all, which looks exactly like a broken export, so
//! `plan` says so before the first file is copied.

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSystem {
    /// FAT12, FAT16 or FAT32: the filesystem type the OS reports does not tell them apart.
    Fat,
    ExFat,
    HfsPlus,
    Apfs,
    Ntfs,
    /// Anything else, by the name the OS gives it.
    Other(String),
}

impl fmt::Display for FileSystem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            FileSystem::Fat => "FAT",
            FileSystem::ExFat => "exFAT",
            FileSystem::HfsPlus => "HFS+",
            FileSystem::Apfs => "APFS",
            FileSystem::Ntfs => "NTFS",
            FileSystem::Other(name) => name,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionTable {
    Mbr,
    Gpt,
}

/// A stick format that rules out some players. The players' operating
/// instructions (CDJ-3000 DRI1586-A p.11, CDJ-2000NXS2 DRI1290-A p.6, and
/// the CDJ-2000NXS, CDJ-900NXS, CDJ-2000, CDJ-900 and CDJ-850 alike) list
/// FAT16, FAT32 and HFS+ and say "NTFS is not supported". exFAT and GPT come
/// from AlphaTheta's support pages: exFAT for the CDJ-3000, XDJ-RX3 and
/// XDJ-XZ only, and no GUID partition map on the CDJ-3000 or the CDJ-2000.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatWarning {
    FileSystem(FileSystem),
    Gpt,
}

impl fmt::Display for FormatWarning {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FormatWarning::FileSystem(FileSystem::ExFat) => f.write_str(
                "This stick is exFAT. A CDJ-3000 reads it (AlphaTheta lists exFAT for the CDJ-3000, XDJ-RX3 and XDJ-XZ); a CDJ-2000NXS2 and older do not, their manuals list FAT16, FAT32 and HFS+ only.",
            ),
            FormatWarning::FileSystem(FileSystem::Ntfs) => f.write_str(
                "This stick is NTFS, which the players' manuals rule out (\"NTFS is not supported\"). Format it as FAT32.",
            ),
            FormatWarning::FileSystem(fs) => write!(
                f,
                "This stick is {fs}, which no player's manual lists (FAT16, FAT32 and HFS+, plus exFAT on a CDJ-3000). Format it as FAT32."
            ),
            FormatWarning::Gpt => f.write_str(
                "This stick has a GUID (GPT) partition table. AlphaTheta says the CDJ-3000 and the CDJ-2000 do not support it, and a CDJ-2000NXS2 is reported not to mount it either. Repartition it with an MBR (Master Boot Record) table.",
            ),
        }
    }
}

/// Warnings for a stick with this filesystem and partition table; nothing for
/// what could not be read.
pub fn warnings(fs: Option<&FileSystem>, table: Option<PartitionTable>) -> Vec<FormatWarning> {
    let mut out = Vec::new();
    if let Some(fs) = fs.filter(|fs| !matches!(fs, FileSystem::Fat | FileSystem::HfsPlus)) {
        out.push(FormatWarning::FileSystem(fs.clone()));
    }
    if table == Some(PartitionTable::Gpt) {
        out.push(FormatWarning::Gpt);
    }
    out
}

/// Filesystem and partition table of the volume `dir` is on, as far as this
/// platform can tell. The partition table belongs to the whole disk rather
/// than the volume, so it is best effort: `None` whenever it cannot be read.
pub fn probe(dir: &Path) -> (Option<FileSystem>, Option<PartitionTable>) {
    imp::probe(dir)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{FileSystem, PartitionTable};
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::process::Command;

    pub fn probe(dir: &Path) -> (Option<FileSystem>, Option<PartitionTable>) {
        let Some(st) = statfs(dir) else {
            return (None, None);
        };
        let fs = match c_str(&st.f_fstypename).as_str() {
            "msdos" => FileSystem::Fat,
            "exfat" => FileSystem::ExFat,
            "hfs" => FileSystem::HfsPlus,
            "apfs" => FileSystem::Apfs,
            "ntfs" => FileSystem::Ntfs,
            other => FileSystem::Other(other.to_string()),
        };
        let table = whole_disk(&c_str(&st.f_mntfromname)).and_then(partition_scheme);
        (Some(fs), table)
    }

    fn statfs(dir: &Path) -> Option<libc::statfs> {
        let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: `path` is NUL-terminated and statfs fills `st` whenever it returns 0.
        (unsafe { libc::statfs(path.as_ptr(), st.as_mut_ptr()) } == 0)
            .then(|| unsafe { st.assume_init() })
    }

    fn c_str(chars: &[libc::c_char]) -> String {
        let bytes: Vec<u8> = chars
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// `/dev/disk4s1` → `disk4`.
    pub(super) fn whole_disk(dev: &str) -> Option<&str> {
        let name = dev.strip_prefix("/dev/")?;
        let digits = name
            .strip_prefix("disk")?
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        (digits > 0).then(|| &name[..4 + digits])
    }

    /// The whole disk's content type from `diskutil`, which a sandboxed app
    /// may not be allowed to run; the caller can fill this in itself then.
    fn partition_scheme(disk: &str) -> Option<PartitionTable> {
        let out = Command::new("diskutil")
            .args(["info", "-plist", disk])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        match plist_string(&String::from_utf8_lossy(&out.stdout), "Content")? {
            "FDisk_partition_scheme" => Some(PartitionTable::Mbr),
            "GUID_partition_scheme" => Some(PartitionTable::Gpt),
            _ => None,
        }
    }

    pub(super) fn plist_string<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
        let rest = &plist[plist.find(&format!("<key>{key}</key>"))?..];
        let start = rest.find("<string>")? + "<string>".len();
        let len = rest[start..].find("</string>")?;
        Some(&rest[start..start + len])
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{FileSystem, PartitionTable};
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::process::Command;

    pub fn probe(dir: &Path) -> (Option<FileSystem>, Option<PartitionTable>) {
        let Ok(dev) = std::fs::metadata(dir).map(|m| m.dev()) else {
            return (None, None);
        };
        let id = format!("{}:{}", libc::major(dev), libc::minor(dev));
        let Some((fstype, source)) = std::fs::read_to_string("/proc/self/mountinfo")
            .ok()
            .and_then(|m| mount_of(&m, &id))
        else {
            return (None, None);
        };
        let fs = match fstype.as_str() {
            "vfat" | "msdos" => Some(FileSystem::Fat),
            "exfat" => Some(FileSystem::ExFat),
            "hfsplus" => Some(FileSystem::HfsPlus),
            "ntfs" | "ntfs3" => Some(FileSystem::Ntfs),
            // exfat-fuse and ntfs-3g both mount as `fuseblk`.
            "fuseblk" => None,
            other => Some(FileSystem::Other(other.to_string())),
        };
        (fs, partition_table(&source))
    }

    /// Filesystem type and source of the mount whose device is `id` (`major:minor`).
    pub(super) fn mount_of(mountinfo: &str, id: &str) -> Option<(String, String)> {
        mountinfo.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.nth(2)? != id {
                return None;
            }
            let mut rest = fields.skip_while(|f| *f != "-").skip(1);
            Some((rest.next()?.to_string(), rest.next()?.to_string()))
        })
    }

    /// `PTTYPE` of the table `device` (a partition such as `/dev/sdb1`) sits in.
    fn partition_table(device: &str) -> Option<PartitionTable> {
        if !device.starts_with("/dev/") {
            return None;
        }
        let out = Command::new("lsblk")
            .args(["-no", "PTTYPE", device])
            .output()
            .ok()?;
        match String::from_utf8_lossy(&out.stdout).trim() {
            "dos" => Some(PartitionTable::Mbr),
            "gpt" => Some(PartitionTable::Gpt),
            _ => None,
        }
    }
}

/// Not read on Windows yet.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::{FileSystem, PartitionTable};
    use std::path::Path;

    pub fn probe(_: &Path) -> (Option<FileSystem>, Option<PartitionTable>) {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fat_and_hfs_plus_on_mbr_are_fine_and_unknowns_say_nothing() {
        assert!(warnings(Some(&FileSystem::Fat), Some(PartitionTable::Mbr)).is_empty());
        assert!(warnings(Some(&FileSystem::HfsPlus), Some(PartitionTable::Mbr)).is_empty());
        assert!(warnings(Some(&FileSystem::Fat), None).is_empty());
        assert!(warnings(None, None).is_empty());
        assert_eq!(
            warnings(Some(&FileSystem::Apfs), None),
            [FormatWarning::FileSystem(FileSystem::Apfs)]
        );
    }

    #[test]
    fn exfat_on_gpt_gets_both_warnings() {
        assert_eq!(
            warnings(Some(&FileSystem::ExFat), Some(PartitionTable::Gpt)),
            [
                FormatWarning::FileSystem(FileSystem::ExFat),
                FormatWarning::Gpt
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn whole_disk_and_plist_parsing() {
        assert_eq!(imp::whole_disk("/dev/disk4s1"), Some("disk4"));
        assert_eq!(imp::whole_disk("/dev/disk12"), Some("disk12"));
        assert_eq!(imp::whole_disk("map auto_home"), None);
        assert_eq!(imp::whole_disk("/dev/diskX"), None);
        let plist = "<dict>\n\t<key>Bootable</key>\n\t<false/>\n\t<key>Content</key>\n\t<string>GUID_partition_scheme</string>\n</dict>";
        assert_eq!(
            imp::plist_string(plist, "Content"),
            Some("GUID_partition_scheme")
        );
        assert_eq!(imp::plist_string(plist, "Missing"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mountinfo_lookup() {
        let m = "22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw\n\
                 91 29 8:17 / /media/dj/USB rw,nosuid shared:50 master:2 - vfat /dev/sdb1 rw,fmask=0022\n";
        assert_eq!(
            imp::mount_of(m, "8:17"),
            Some(("vfat".into(), "/dev/sdb1".into()))
        );
        assert_eq!(imp::mount_of(m, "8:1"), None);
    }
}
