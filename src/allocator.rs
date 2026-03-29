use bump_into::{self, BumpInto};

/// Type alias for the bump allocator used for dynamic memory management in the HTTP servers
pub type HttpAllocator<'a> = BumpInto<'a>;
