//! What the stick is formatted as (issue #184). A player that cannot mount a
//! stick shows no library at all, which looks exactly like a broken export, so
//! `plan` says so before the first file is copied. Also how much it has free
//! (issue #233).

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSystem {
    /// FAT12, FAT16 or FAT32: the filesystem type the OS reports does not tell them apart.
    Fat,
    ExFat,
    HfsPlus,
    /// HFS+ formatted case-sensitive (HFSX, Disk Utility's "Mac OS Extended
    /// (Case-sensitive)"). Only told apart on macOS.
    HfsPlusCaseSensitive,
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
            FileSystem::HfsPlusCaseSensitive => "case-sensitive HFS+",
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
/// The same pages (OPUS-QUAD, XDJ-RX3, XDJ-AZ, CDJ-3000X) say a device
/// formatted "Mac Extended format (case-sensitive) might not be recognized".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatWarning {
    FileSystem(FileSystem),
    Gpt,
    /// A FAT filesystem in an MBR partition whose type is not a FAT one, by
    /// its type id (issue #288).
    PartitionType(u8),
}

/// MBR partition type ids of the FAT family: FAT12, FAT16 under 32 MB,
/// FAT16, FAT32, FAT32 with LBA, FAT16 with LBA.
pub const FAT_PARTITION_TYPES: [u8; 6] = [0x01, 0x04, 0x06, 0x0b, 0x0c, 0x0e];

impl fmt::Display for FormatWarning {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FormatWarning::FileSystem(FileSystem::ExFat) => f.write_str(
                "This stick is exFAT. A CDJ-3000 reads it (AlphaTheta lists exFAT for the CDJ-3000, XDJ-RX3 and XDJ-XZ); a CDJ-2000NXS2 and older do not, their manuals list FAT16, FAT32 and HFS+ only.",
            ),
            FormatWarning::FileSystem(FileSystem::HfsPlusCaseSensitive) => f.write_str(
                "This stick is case-sensitive HFS+. AlphaTheta says a device formatted Mac OS Extended (Case-sensitive) might not be recognised. Format it as FAT32, or as Mac OS Extended (Journaled), which is not case-sensitive.",
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
            FormatWarning::PartitionType(id) => write!(
                f,
                "This stick is FAT, but its partition has type 0x{id:02x}{}, not a FAT type. Two XDJ-700s (firmware 1.15) showed FORMAT USB for such a stick and read it once the type was set to 0x0c (W95 FAT32 LBA), with nothing else changed. To set it: sudo fdisk <the stick's disk, such as /dev/sdb>, then t, c, w.",
                if *id == 0x83 { " (Linux)" } else { "" }
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

/// A warning for a FAT stick whose MBR partition type is not a FAT one
/// (issue #288). `mkfs.vfat` on a partition `fdisk` or `parted` made keeps
/// its type, 0x83 by default, and Linux mounts it regardless; macOS mounts
/// FAT on a FAT type only (measured on disk images: 0x07, 0x83 and 0xaf were
/// not mounted), so only Linux reads `id`. Nothing when it is unknown.
pub fn partition_type_warning(
    fs: Option<&FileSystem>,
    table: Option<PartitionTable>,
    id: Option<u8>,
) -> Option<FormatWarning> {
    let id = id.filter(|id| !FAT_PARTITION_TYPES.contains(id))?;
    (fs == Some(&FileSystem::Fat) && table == Some(PartitionTable::Mbr))
        .then_some(FormatWarning::PartitionType(id))
}

/// The MBR type id of the partition the volume `dir` is on: Linux only,
/// from `lsblk`; `None` elsewhere, on a GPT disk and where it cannot be read.
pub fn partition_type(dir: &Path) -> Option<u8> {
    imp::partition_type(dir)
}

/// Free space on the volume `dir` is on, and the unit it allocates files in
/// (the cluster on FAT and exFAT), from `statvfs` (issue #233).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Space {
    /// Bytes an ordinary user can still write.
    pub available: u64,
    pub unit: u64,
}

/// `None` where it cannot be read, and on Windows, where it is not read yet.
pub(crate) fn space(dir: &Path) -> Option<Space> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `path` is NUL-terminated and statvfs fills `st` whenever it returns 0.
        if unsafe { libc::statvfs(path.as_ptr(), st.as_mut_ptr()) } != 0 {
            return None;
        }
        let st = unsafe { st.assume_init() };
        // the field types differ between platforms
        #[allow(clippy::unnecessary_cast)]
        let (unit, blocks) = (st.f_frsize as u64, st.f_bavail as u64);
        (unit > 0).then(|| Space {
            available: blocks.saturating_mul(unit),
            unit,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
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
            "hfs" if case_sensitive(dir) => FileSystem::HfsPlusCaseSensitive,
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

    fn case_sensitive(dir: &Path) -> bool {
        let Ok(path) = CString::new(dir.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: `path` is NUL-terminated.
        unsafe { libc::pathconf(path.as_ptr(), libc::_PC_CASE_SENSITIVE) == 1 }
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

    pub fn partition_type(_: &Path) -> Option<u8> {
        None
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

    /// Filesystem type and source device of the mount `dir` is on.
    fn mount(dir: &Path) -> Option<(String, String)> {
        let dev = std::fs::metadata(dir).ok()?.dev();
        let id = format!("{}:{}", libc::major(dev), libc::minor(dev));
        mount_of(&std::fs::read_to_string("/proc/self/mountinfo").ok()?, &id)
    }

    pub fn partition_type(dir: &Path) -> Option<u8> {
        let (_, source) = mount(dir)?;
        if !source.starts_with("/dev/") {
            return None;
        }
        let out = Command::new("lsblk")
            .args(["-no", "PARTTYPE", &source])
            .output()
            .ok()?;
        parse_partition_type(&String::from_utf8_lossy(&out.stdout))
    }

    /// `lsblk`'s `PARTTYPE` of an MBR partition, such as `0xc`; a GPT
    /// partition's is a GUID, which is not one.
    pub(super) fn parse_partition_type(s: &str) -> Option<u8> {
        u8::from_str_radix(s.trim().strip_prefix("0x")?, 16).ok()
    }

    pub fn probe(dir: &Path) -> (Option<FileSystem>, Option<PartitionTable>) {
        let Some((fstype, source)) = mount(dir) else {
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

    pub fn partition_type(_: &Path) -> Option<u8> {
        None
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
    fn case_sensitive_hfs_plus_is_warned_about() {
        assert_eq!(
            warnings(
                Some(&FileSystem::HfsPlusCaseSensitive),
                Some(PartitionTable::Mbr)
            ),
            [FormatWarning::FileSystem(FileSystem::HfsPlusCaseSensitive)]
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

    /// The XDJ-700 report: FAT32 on type 0x83 (#288).
    #[test]
    fn a_fat_stick_on_another_partition_type_is_warned_about() {
        let fat = Some(&FileSystem::Fat);
        let mbr = Some(PartitionTable::Mbr);
        assert_eq!(
            partition_type_warning(fat, mbr, Some(0x83)),
            Some(FormatWarning::PartitionType(0x83))
        );
        assert!(FormatWarning::PartitionType(0x83)
            .to_string()
            .contains("0x83 (Linux)"));
        for id in FAT_PARTITION_TYPES {
            assert_eq!(partition_type_warning(fat, mbr, Some(id)), None);
        }
        assert_eq!(partition_type_warning(fat, mbr, None), None);
        assert_eq!(
            partition_type_warning(fat, Some(PartitionTable::Gpt), Some(0x83)),
            None
        );
        // exFAT on 0x07 is how it should be
        assert_eq!(
            partition_type_warning(Some(&FileSystem::ExFat), mbr, Some(0x07)),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn lsblk_partition_types() {
        assert_eq!(imp::parse_partition_type("0xc\n"), Some(0x0c));
        assert_eq!(imp::parse_partition_type(" 0x83"), Some(0x83));
        assert_eq!(
            imp::parse_partition_type("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7"),
            None
        );
        assert_eq!(imp::parse_partition_type(""), None);
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
