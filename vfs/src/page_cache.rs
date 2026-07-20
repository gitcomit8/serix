/*
 * page_cache.rs - Unified Page Cache
 *
 * Radix tree indexed by (InodeId, page_offset) with LRU eviction.
 * Pages are PAGE_SIZE (4096) byte blocks. Dirty pages are written back
 * on demand or when the cache is full.
 *
 * Design:
 *   - BTreeMap for the radix tree (ordered, no external deps)
 *   - VecDeque for LRU list (evict least recently used)
 *   - Pages are Arc<Mutex<Page>> for shared ownership
 *   - Accessed_at tracks LRU order (updated on every get)
 */

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub const PAGE_SIZE: usize = 4096;

/* ------------------------------------------------------------------ */
/*  Page                                                                */
/* ------------------------------------------------------------------ */

/*
 * struct Page - One page in the cache
 */
pub struct Page {
	/* Raw page data */
	data: [u8; PAGE_SIZE],
	/* Backing inode ID */
	inode_id: u32,
	/* Page offset within the file (in pages) */
	offset: u32,
	/* True if page has been modified */
	dirty: bool,
}

impl Page {
	pub fn new(inode_id: u32, offset: u32, data: &[u8; PAGE_SIZE]) -> Self {
		Self {
			data: *data,
			inode_id,
			offset,
			dirty: false,
		}
	}

	pub fn data(&self) -> &[u8; PAGE_SIZE] {
		&self.data
	}

	pub fn data_mut(&mut self) -> &mut [u8; PAGE_SIZE] {
		self.dirty = true;
		&mut self.data
	}

	pub fn mark_dirty(&mut self) {
		self.dirty = true;
	}

	pub fn mark_clean(&mut self) {
		self.dirty = false;
	}

	pub fn is_dirty(&self) -> bool {
		self.dirty
	}
}

/* ------------------------------------------------------------------ */
/*  Page Cache                                                          */
/* ------------------------------------------------------------------ */

/* Maximum number of pages in the cache */
const MAX_CACHE_PAGES: usize = 256;

/*
 * struct PageCache - In-memory page cache
 *
 * Key: (inode_id, page_offset) — maps to a Page.
 * LRU list tracks access order for eviction.
 */
pub struct PageCache {
	/* Radix tree: (InodeId, page_offset) -> Page */
	tree: Mutex<BTreeMap<(u32, u32), Arc<Mutex<Page>>>>,
	/* LRU list: most recently used at back */
	lru: Mutex<VecDeque<(u32, u32)>>,
	/* Maximum pages */
	max_pages: usize,
}

impl PageCache {
	pub fn new(max_pages: usize) -> Self {
		Self {
			tree: Mutex::new(BTreeMap::new()),
			lru: Mutex::new(VecDeque::new()),
			max_pages,
		}
	}

	/*
	 * get_page - Look up a page in the cache
	 *
	 * Returns Some(Arc<Mutex<Page>>) if found, None if miss.
	 * On hit, moves the page to the back of the LRU list (most recent).
	 */
	pub fn get_page(&self, inode_id: u32, page_offset: u32) -> Option<Arc<Mutex<Page>>> {
		let key = (inode_id, page_offset);
		let mut tree = self.tree.lock();
		let mut lru = self.lru.lock();

		if let Some(page) = tree.get(&key) {
			/* Move to back of LRU (most recent) */
			lru.retain(|k| *k != key);
			lru.push_back(key);
			Some(Arc::clone(page))
		} else {
			None
		}
	}

	/*
	 * insert_page - Insert a page into the cache
	 *
	 * Evicts LRU pages if the cache is full.
	 */
	pub fn insert_page(&self, page: Page) {
		let key = (page.inode_id, page.offset);
		let arc = Arc::new(Mutex::new(page));

		let mut tree = self.tree.lock();
		let mut lru = self.lru.lock();

		/* Evict LRU pages if full */
		while tree.len() >= self.max_pages {
			if let Some(evict_key) = lru.pop_front() {
				tree.remove(&evict_key);
			} else {
				break;
			}
		}

		tree.insert(key, Arc::clone(&arc));
		lru.push_back(key);
	}

	/*
	 * writeback_dirty_pages - Write all dirty pages back to disk
	 *
	 * Calls the provided writeback function for each dirty page.
	 * The writeback function is responsible for writing the page data
	 * to the underlying block device.
	 */
	pub fn writeback_dirty_pages<F>(&self, mut writeback: F)
	where
		F: FnMut(u32, u32, &[u8; PAGE_SIZE]),
	{
		let tree = self.tree.lock();
		for ((inode_id, offset), page) in tree.iter() {
			let p = page.lock();
			if p.is_dirty() {
				writeback(*inode_id, *offset, p.data());
				/* Mark clean after writeback */
				let mut p = page.lock();
				p.mark_clean();
			}
		}
	}

	/*
	 * invalidate_inode - Remove all pages for a given inode
	 *
	 * Called when an inode is truncated or unlinked.
	 */
	pub fn invalidate_inode(&self, inode_id: u32) {
		let mut tree = self.tree.lock();
		let mut lru = self.lru.lock();

		let mut to_remove = Vec::new();
		for key in tree.keys() {
			if key.0 == inode_id {
				to_remove.push(*key);
			}
		}
		for key in to_remove {
			tree.remove(&key);
			lru.retain(|k| *k != key);
		}
	}

	/*
	 * count - Number of pages in the cache
	 */
	pub fn count(&self) -> usize {
		self.tree.lock().len()
	}
}

/* ------------------------------------------------------------------ */
/*  Global page cache instance                                          */
/* ------------------------------------------------------------------ */

static PAGE_CACHE_INSTANCE: spin::Once<PageCache> = spin::Once::new();

/*
 * init_page_cache - Initialize the global page cache
 *
 * Must be called once during boot, before any VFS operations.
 */
pub fn init_page_cache(max_pages: usize) {
	PAGE_CACHE_INSTANCE.call_once(|| PageCache::new(max_pages));
}

/*
 * get_page_cache - Get a reference to the global page cache
 */
pub fn get_page_cache() -> Option<&'static PageCache> {
	PAGE_CACHE_INSTANCE.get()
}

/*
 * read_with_cache - Read from a file, using the page cache
 *
 * @inode: INode to read from (must implement ino())
 * @offset: Byte offset to read from
 * @buf: Output buffer
 * @disk_read: Function to read raw data from disk on cache miss
 *
 * Returns the number of bytes read.
 */
pub fn read_with_cache<F>(
	inode_id: u32,
	offset: usize,
	buf: &mut [u8],
	inode_size: usize,
	mut disk_read: F,
) -> usize
where
	F: FnMut(usize, &mut [u8]) -> usize,
{
	if buf.is_empty() {
		return 0;
	}

	/* Clamp to file size */
	let available = inode_size.saturating_sub(offset);
	if available == 0 {
		return 0;
	}
	let len = core::cmp::min(buf.len(), available);

	let cache = match get_page_cache() {
		Some(c) => c,
		None => return disk_read(offset, &mut buf[..len]),
	};

	let page_offset = (offset / PAGE_SIZE) as u32;
	let page_offset_in_block = offset % PAGE_SIZE;
	let mut done = 0usize;
	let mut cur_page = page_offset;

	while done < len {
		let start_in_page = if cur_page == page_offset {
			page_offset_in_block
		} else {
			0
		};
		let remaining_in_page = PAGE_SIZE - start_in_page;
		let to_copy = core::cmp::min(len - done, remaining_in_page);

		if let Some(page) = cache.get_page(inode_id, cur_page) {
			/* Cache hit */
			let p = page.lock();
			buf[done..done + to_copy].copy_from_slice(&p.data()[start_in_page..start_in_page + to_copy]);
			done += to_copy;
		} else {
			/* Cache miss: read from disk */
			let disk_offset = (cur_page as usize) * PAGE_SIZE;
			let mut page_data = [0u8; PAGE_SIZE];
			let read_len = core::cmp::min(PAGE_SIZE, inode_size.saturating_sub(disk_offset));
			if read_len > 0 {
				disk_read(disk_offset, &mut page_data[..read_len]);
			}
			let page = Page::new(inode_id, cur_page, &page_data);
			cache.insert_page(page);

			/* Copy from the newly inserted page */
			let page = cache.get_page(inode_id, cur_page).unwrap();
			let p = page.lock();
			buf[done..done + to_copy].copy_from_slice(&p.data()[start_in_page..start_in_page + to_copy]);
			done += to_copy;
		}

		cur_page += 1;
	}

	done
}

/*
 * write_with_cache - Write to a file, using the page cache
 *
 * @inode_id: Inode ID
 * @offset: Byte offset to write to
 * @buf: Data to write
 * @disk_write: Function to flush dirty pages to disk
 */
pub fn write_with_cache<F>(
	inode_id: u32,
	offset: usize,
	buf: &[u8],
	mut disk_write: F,
) -> usize
where
	F: FnMut(),
{
	if buf.is_empty() {
		return 0;
	}

	let cache = match get_page_cache() {
		Some(c) => c,
		None => {
			disk_write();
			return buf.len();
		}
	};

	let mut done = 0usize;
	let mut cur_page = (offset / PAGE_SIZE) as u32;

	while done < buf.len() {
		let start_in_page = if cur_page == (offset / PAGE_SIZE) as u32 {
			offset % PAGE_SIZE
		} else {
			0
		};
		let remaining_in_page = PAGE_SIZE - start_in_page;
		let to_copy = core::cmp::min(buf.len() - done, remaining_in_page);

		/* Get or create page */
		let page = if let Some(page) = cache.get_page(inode_id, cur_page) {
			page
		} else {
			let page_data = [0u8; PAGE_SIZE];
			let page = Page::new(inode_id, cur_page, &page_data);
			cache.insert_page(page);
			cache.get_page(inode_id, cur_page).unwrap()
		};

		/* Write data to page */
		{
			let mut p = page.lock();
			let data = p.data_mut();
			data[start_in_page..start_in_page + to_copy]
				.copy_from_slice(&buf[done..done + to_copy]);
		}

		done += to_copy;
		cur_page += 1;
	}

	/* Flush dirty pages to disk */
	disk_write();

	done
}
