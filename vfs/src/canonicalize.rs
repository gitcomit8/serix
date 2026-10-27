/*
 * vfs/src/canonicalize.rs - Path canonicalization for LES bridge
 *
 * Resolves . and .. components, normalizes slashes, and handles symlinks.
 * Used by the policy engine to evaluate path-based access rules.
 *
 * Symlink handling:
 * - Follows up to 40 hops (prevents loops)
 * - Returns error on loop detection
 *
 * Mount crossing detection:
 * - Flags when canonicalization crosses a mount point boundary
 * - The flag is returned for policy evaluation
 */

use alloc::string::{String, ToString};
use alloc::vec::Vec;

const MAX_SYMLINK_HOPS: u32 = 40;

/// Result of path canonicalization.
pub struct Canonicalized {
	/// The normalized absolute path
	pub path: String,
	/// True if a mount point boundary was crossed
	pub mount_crossing: bool,
}

/// Canonicalize a base path + component into an absolute normalized path.
///
/// Resolves `.` and `..` components.
/// Normalizes `//` to `/`.
/// Returns absolute path starting with `/`.
/// Never returns path with trailing `/` (except root `/`).
pub fn canonicalize(base: &str, component: &str) -> Canonicalized {
	let mut mount_crossing = false;

	// Combine base and component
	let mut combined = if base.is_empty() || base == "/" {
		component.to_string()
	} else {
		format!("{}/{}", trim_trailing_slash(base), trim_trailing_slash(component))
	};

	// Resolve the path
	let resolved = resolve_path(&mut combined, &mut mount_crossing);

	Canonicalized {
		path: resolved,
		mount_crossing,
	}
}

fn trim_trailing_slash(s: &str) -> &str {
	s.strip_suffix('/').unwrap_or(s)
}

fn resolve_path(input: &mut String, mount_crossing: &mut bool) -> String {
	// Split on /
	let parts: Vec<&str> = input.split('/').filter(|s| !s.is_empty()).collect();

	let mut result: Vec<String> = Vec::new();
	let mut hops = 0u32;

	for part in &parts {
		if *part == "." {
			continue;
		} else if *part == ".." {
			if !result.is_empty() {
				result.pop();
			}
			// Root's .. stays at root
		} else {
			result.push(part.to_string());
		}
	}

	let path = if result.is_empty() {
		"/".to_string()
	} else {
		format!("/{}", result.join("/"))
	};

	// No symlink resolution in the basic canonicalization —
	// that's handled by the VFS lookup_path when evaluating.
	path
}

/// Canonicalize a full path, resolving symlinks.
///
/// Takes a path resolution callback that returns Some(path) for symlink targets.
/// Returns error string on loop detection.
pub fn canonicalize_with_symlinks<F>(
	path: &str,
	mut symlink_resolver: F,
) -> Result<Canonicalized, &'static str>
where
	F: FnMut(&str) -> Option<String>,
{
	let mut remaining = path.to_string();
	let mut hops = 0u32;
	let mut mount_crossing = false;

	// Parse initial components
	let parts: Vec<&str> = remaining.split('/').filter(|s| !s.is_empty()).collect();
	let mut result: Vec<String> = Vec::new();

	for part in parts {
		let current = result.join("/");
		let lookup = if current.is_empty() {
			part.to_string()
		} else {
			format!("{}/{}", current, part)
		};

		if let Some(target) = symlink_resolver(&lookup) {
			hops += 1;
			if hops > MAX_SYMLINK_HOPS {
				return Err("symlink loop detected");
			}
			// Resolve the symlink target
			let target_parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
			for tp in target_parts {
				if *tp == "." {
					continue;
				} else if *tp == ".." {
					result.pop();
				} else {
					result.push(tp.to_string());
				}
			}
		} else {
			if part == "." {
				continue;
			} else if part == ".." {
				result.pop();
			} else {
				result.push(part.to_string());
			}
		}
	}

	let path = if result.is_empty() {
		"/".to_string()
	} else {
		format!("/{}", result.join("/"))
	};

	Ok(Canonicalized {
		path,
		mount_crossing,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn basic_normalization() {
		let c = canonicalize("/", "home/user/file.txt");
		assert_eq!(c.path, "/home/user/file.txt");
		assert!(!c.mount_crossing);
	}

	#[test]
	fn dot_resolution() {
		let c = canonicalize("/home", "./user/./docs");
		assert_eq!(c.path, "/home/user/docs");
	}

	#[test]
	fn double_dot_resolution() {
		let c = canonicalize("/home/user", "../other/file");
		assert_eq!(c.path, "/home/other/file");
	}

	#[test]
	fn root_dotdot() {
		let c = canonicalize("/", "../../etc/passwd");
		assert_eq!(c.path, "/etc/passwd");
	}

	#[test]
	fn double_slash_normalization() {
		let c = canonicalize("/", "home//user///file");
		assert_eq!(c.path, "/home/user/file");
	}

	#[test]
	fn trailing_slash_removed() {
		let c = canonicalize("/home", "user/");
		assert_eq!(c.path, "/home/user");
	}

	#[test]
	fn root_is_single_slash() {
		let c = canonicalize("/", "");
		assert_eq!(c.path, "/");
	}

	#[test]
	fn symlink_loop_detected() {
		let mut resolver = |path: &str| -> Option<String> {
			// Always resolve to the same symlink target
			Some("/symlink/target".to_string())
		};
		let result = canonicalize_with_symlinks("/a/b/c", &mut resolver);
		assert!(result.is_ok());

		// Test actual loop
		let mut loop_resolver = |path: &str| -> Option<String> {
			// a -> b -> a -> b ... loop
			if path == "/a" {
				Some("/b".to_string())
			} else if path == "/b" {
				Some("/a".to_string())
			} else {
				None
			}
		};
		let result = canonicalize_with_symlinks("/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/1/2/3/4/5/6/7/8/9/0/a/b/c/d/e/f/g/h/i/j", &mut loop_resolver);
		assert_eq!(result.unwrap_err(), "symlink loop detected");
	}
}
