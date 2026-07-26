/*
 * init/filesystem.rs - Filesystem & VFS Initialization
 *
 * Provides init_filesystem() which handles:
 * - Registering filesystem drivers (FAT32, ext2, ext4)
 * - Initializing VFS mount table (/ and /dev/)
 * - Setting up page cache
 * - Registering block devices and creating /dev/sda
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

	/* Initialize VMA manager for mmap */
	unsafe { memory::vma::init_vma_manager() };

	/* Expose VirtIO block device as /dev/sda */
	let sda_dev = Arc::new(fs::VirtioBlockDev);
	fs::register_block_device(
		"/dev/sda",
		Arc::clone(&sda_dev) as Arc<dyn fs::BlockDev>,
	);
	let sda: Arc<dyn INode> = Arc::new(fs::BlockDevINode(sda_dev));
	if let Some(dev_dir) = vfs::lookup_path("/dev/") {
		dev_dir.insert("sda", sda).ok();
		serial_println!("VFS: /dev/sda available");
		graphics::fb_println!("VFS: /dev/sda ready — run 'mount /dev/sda /' to attach ext2");
	}

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
}
