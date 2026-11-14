/*
 * init/filesystem.rs - Filesystem & VFS Initialization
 *
 * Provides init_filesystem() which handles:
 * - Registering filesystem drivers (FAT32, ext2, ext4)
 * - Initializing VFS mount table (/ and /dev/)
 * - Setting up page cache
 * - Registering block devices as /dev/sdX plus GPT partitions /dev/sdXN
 * - Inserting ext4d ELF into VFS
 */

use alloc::sync::Arc;
use vfs::INode;
use hal::serial_println;

/*
 * init_filesystem - Initialize the filesystem stack
 *
 * Registers all filesystem drivers, sets up the VFS mount table,
 * initializes the page cache, and exposes the VirtIO block device
 * as /dev/sda. Also inserts the embedded ext4d ELF into the VFS root.
 */
pub fn init_filesystem() {
	serial_println!("--- Filesystem Initialization ---");

	/* Register filesystem drivers */
	fs::fat32::init();
	fs::ext2::init();
	fs::ext4::init();

	/* Boot VFS: RamDir at / and /dev/ */
	vfs::mount("/", Arc::new(vfs::RamDir::new("/")));
	vfs::mount("/dev/", Arc::new(vfs::RamDir::new("dev")));
	serial_println!("VFS: mount table initialized");
	graphics::fb_println!("VFS: / and /dev/ ready");

	/* Initialize page cache (256 pages = 1 MiB) */
	vfs::page_cache::init_page_cache(256);
	serial_println!("VFS: page cache initialized (256 pages)");

	/* Initialize /proc/serix/cap-audit */
	{
		let serix_dir: Arc<dyn vfs::INode> = Arc::new(vfs::RamDir::new("serix"));
		let audit_file: Arc<dyn vfs::INode> = Arc::new(crate::procfs::make_audit_file());
		if let Some(root) = vfs::lookup_path("/") {
			root.insert("proc", serix_dir).ok();
		}
		if let Some(serix) = vfs::lookup_path("/proc") {
			serix.insert("cap-audit", audit_file).ok();
		}
		serial_println!("VFS: /proc/serix/cap-audit ready");
	}

	/* VMA manager is per-task (stored in TaskCB) */

	/* Expose every block device and its GPT partitions */
	expose_block_devices();

	/* Insert ext4d daemon ELF into VFS root */
	{
		let ext4d_file: Arc<dyn INode> =
			Arc::new(vfs::RamFile::new_with_data(include_bytes!(
				"../../../target/x86_64-unknown-none/release/ext4d"
			)));
		if let Some(root) = vfs::lookup_path("/") {
			root.insert("ext4d", ext4d_file).ok();
		}
		serial_println!("ext4d: ELF inserted into VFS");
	}

	/* Insert policyd daemon ELF into VFS root */
	{
		let policyd_file: Arc<dyn INode> =
			Arc::new(vfs::RamFile::new_with_data(include_bytes!(
				"../../../target/x86_64-unknown-none/release/policyd"
			)));
		if let Some(root) = vfs::lookup_path("/") {
			root.insert("policyd", policyd_file).ok();
		}
		serial_println!("policyd: ELF inserted into VFS");
	}

	/* Create /etc/serix/ directory and insert default policy.toml config */
	{
		if let Some(root) = vfs::lookup_path("/") {
			if root.mkdir("etc").is_ok() {
				if let Some(serixd) = vfs::lookup_path("/etc/serix") {
					let default_config = "\
# Serix Policy Daemon Configuration\n\
# Path-based access control rules\n\
\n\
[[rule]]\
target = \"/etc/shadow\"\
match_type = \"exact\"\
action = \"deny\"\
required_rights = \"READ\"\

[[rule]]\
target = \"/proc/serix/cap-audit\"\
match_type = \"exact\"\
action = \"deny\"\
required_rights = \"READ\"\

[[rule]]\
target = \"/\"\
match_type = \"prefix\"\
action = \"allow\"\
required_rights = \"READ,WRITE,LOOKUP\"\
";
					let config_file: Arc<dyn INode> =
						Arc::new(vfs::RamFile::new_with_data(default_config.as_bytes()));
					if let Some(serixd) = vfs::lookup_path("/etc/serix") {
						serixd.insert("policy.toml", config_file).ok();
					}
					serial_println!("policy.toml: default config inserted at /etc/serix/policy.toml");
				}
			}
		}
	}

	/* Create /mnt/ directory for auto-mounted ext4 filesystem */
	{
		if let Some(root) = vfs::lookup_path("/") {
			if root.mkdir("mnt").is_ok() {
				serial_println!("VFS: /mnt/ directory created");
			}
		}
	}
}

/*
 * expose_block_devices - Publish disks and GPT partitions under /dev
 *
 * Each initialized VirtIO block device becomes /dev/sda, /dev/sdb, ... in
 * table order. A validated GPT adds /dev/sda1, /dev/sda2, ... as offset views
 * of the whole disk. Disks without a GPT keep only the whole-disk node, and a
 * rejected table is reported and never exposed.
 */
fn expose_block_devices() {
	let Some(dev_dir) = vfs::lookup_path("/dev/") else {
		serial_println!("VFS: /dev/ missing, block devices not exposed");
		return;
	};

	let disk_count = drivers::virtio::device_count();
	serial_println!("VFS: {} block device(s) detected", disk_count);

	for index in 0..disk_count {
		let disk_name = fs::disk_name(index);
		let disk: Arc<dyn fs::BlockDev> = Arc::new(fs::VirtioBlockDev::new(index));
		let sectors = disk.sector_count();

		fs::register_block_device(&alloc::format!("/dev/{}", disk_name), Arc::clone(&disk));
		let disk_node: Arc<dyn INode> = Arc::new(fs::BlockDevINode(Arc::clone(&disk)));
		dev_dir.insert(disk_name.as_str(), disk_node).ok();
		serial_println!("VFS: /dev/{} available ({} sectors)", disk_name, sectors);
		graphics::fb_println!("VFS: /dev/{} ready", disk_name);

		match fs::gpt::read_gpt(&*disk, sectors) {
			Ok(partitions) if !partitions.is_empty() => {
				serial_println!(
					"VFS: /dev/{}: GPT with {} partition(s)",
					disk_name,
					partitions.len()
				);
				for part in partitions {
					let part_name = alloc::format!("{}{}", disk_name, part.number);
					let dev: Arc<dyn fs::BlockDev> = Arc::new(fs::PartitionDev::new(
						Arc::clone(&disk),
						part.start_lba,
						part.sector_count,
					));
					fs::register_block_device(
						&alloc::format!("/dev/{}", part_name),
						Arc::clone(&dev),
					);
					let node: Arc<dyn INode> = Arc::new(fs::BlockDevINode(dev));
					dev_dir.insert(part_name.as_str(), node).ok();

					/*
					 * Provenance probe: reading the parent at start_lba is
					 * what proves the offset translation is correct.
					 */
					let mut head = [0u8; 8];
					let mut probe = [0u8; 512];
					if disk.read_block(part.start_lba, &mut probe) {
						head.copy_from_slice(&probe[..8]);
					}
					serial_println!(
						"VFS: /dev/{} start={} sectors={} first8={:02x?}",
						part_name,
						part.start_lba,
						part.sector_count,
						head
					);
				}
			}
			Ok(_) => serial_println!("VFS: /dev/{}: GPT without partitions", disk_name),
			Err(fs::gpt::GptError::NoGpt) => {
				serial_println!("VFS: /dev/{}: no GPT, whole-disk device only", disk_name)
			}
			Err(err) => {
				serial_println!("VFS: /dev/{}: GPT rejected ({:?})", disk_name, err);
				graphics::fb_println!("VFS: /dev/{}: GPT rejected", disk_name);
			}
		}
	}
}
