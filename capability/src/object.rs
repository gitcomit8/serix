/*
 * Object Identifiers
 *
 * Typed object IDs used in capability records. Each newtype wraps a u64
 * and provides From/Into conversions at subsystem boundaries only.
 * Prevents passing a PortId where an InodeId is expected.
 */

use core::fmt;

/* ------------------------------------------------------------------ */
/*  ObjectId — generic object identifier                               */
/* ------------------------------------------------------------------ */

/// Generic object identifier. Concrete types are newtypes below.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

impl fmt::Debug for ObjectId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Obj({})", self.0)
	}
}

/* ------------------------------------------------------------------ */
/*  PortId — IPC port identifier                                       */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortId(pub u64);

impl fmt::Debug for PortId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Port({})", self.0)
	}
}

impl From<PortId> for u64 {
	fn from(id: PortId) -> Self {
		id.0
	}
}

/* ------------------------------------------------------------------ */
/*  InodeId — filesystem inode identifier                              */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InodeId(pub u64);

impl fmt::Debug for InodeId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Inode({})", self.0)
	}
}

impl From<InodeId> for u64 {
	fn from(id: InodeId) -> Self {
		id.0
	}
}

/* ------------------------------------------------------------------ */
/*  DeviceId — device identifier (PCI BDF encoded)                     */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub u64);

impl fmt::Debug for DeviceId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Dev({})", self.0)
	}
}

impl From<DeviceId> for u64 {
	fn from(id: DeviceId) -> Self {
		id.0
	}
}

/* ------------------------------------------------------------------ */
/*  FrameRangeId — memory region identifier (start+length)             */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameRangeId(pub u64);

impl fmt::Debug for FrameRangeId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Frame({})", self.0)
	}
}

impl From<FrameRangeId> for u64 {
	fn from(id: FrameRangeId) -> Self {
		id.0
	}
}

/* ------------------------------------------------------------------ */
/*  TaskId — task/process identifier                                   */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl fmt::Debug for TaskId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "Task({})", self.0)
	}
}

impl From<TaskId> for u64 {
	fn from(id: TaskId) -> Self {
		id.0
	}
}
