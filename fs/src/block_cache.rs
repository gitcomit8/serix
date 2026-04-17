/*
 * block_cache.rs - Write-through sector cache
 *
 * Wraps any BlockDev with a fixed-capacity LRU-ish cache.
 * Capacity is measured in 512-byte sectors; eviction is FIFO.
 */

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use crate::BlockDev;

const CACHE_CAP: usize = 512; /* 512 sectors = 256 KiB */

struct Inner {
	cache: BTreeMap<u64, [u8; 512]>,
	order: Vec<u64>,          /* insertion order for FIFO eviction */
	dev:   Arc<dyn BlockDev>,
}

pub struct CachedBlockDev {
	inner: Mutex<Inner>,
}

impl CachedBlockDev {
	pub fn new(dev: Arc<dyn BlockDev>) -> Self {
		Self {
			inner: Mutex::new(Inner {
				cache: BTreeMap::new(),
				order: Vec::new(),
				dev,
			}),
		}
	}

	/* flush - Write all dirty sectors (no-op for write-through; kept for API symmetry) */
	pub fn flush(&self) { /* write-through: nothing to do */ }
}

impl BlockDev for CachedBlockDev {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		let mut g = self.inner.lock();
		if let Some(line) = g.cache.get(&sector) {
			buf.copy_from_slice(line);
			return true;
		}
		/* Cache miss */
		if !g.dev.read_block(sector, buf) { return false; }
		/* Evict FIFO if full */
		if g.cache.len() >= CACHE_CAP {
			if let Some(old) = g.order.first().copied() {
				g.cache.remove(&old);
				g.order.remove(0);
			}
		}
		g.cache.insert(sector, *buf);
		g.order.push(sector);
		true
	}

	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		let mut g = self.inner.lock();
		/* Write-through: device first */
		if !g.dev.write_block(sector, buf) { return false; }
		/* Update cache entry if present; insert if there is room */
		if let Some(line) = g.cache.get_mut(&sector) {
			line.copy_from_slice(buf);
		} else if g.cache.len() < CACHE_CAP {
			g.cache.insert(sector, *buf);
			g.order.push(sector);
		}
		true
	}

	fn sector_count(&self) -> u64 {
		self.inner.lock().dev.sector_count()
	}
}
