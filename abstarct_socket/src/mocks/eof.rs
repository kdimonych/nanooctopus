use crate::read_stream::IntoReadError;

/// EOF error for the dummy read stream
#[derive(Debug)]
pub struct EOF;

impl IntoReadError for EOF {}
