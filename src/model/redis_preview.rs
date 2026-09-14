use crate::value_preview::{PreviewFormat, ValueEncoding, ValueView};

/// UI-facing selection state.  Raw bytes and decoded payloads stay outside
/// this small, cloneable model until the runtime pipeline is introduced.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RedisPreviewFormatState {
    pub selected: PreviewFormat,
    pub automatic: bool,
}

impl Default for RedisPreviewFormatState {
    fn default() -> Self {
        Self {
            selected: PreviewFormat::RAW,
            automatic: true,
        }
    }
}

impl RedisPreviewFormatState {
    pub fn select(&mut self, format: PreviewFormat) {
        self.selected = format;
        self.automatic = false;
    }

    pub fn reset_auto(&mut self) {
        self.selected = PreviewFormat::RAW;
        self.automatic = true;
    }

    pub fn encoding(&self) -> ValueEncoding {
        self.selected.encoding
    }

    pub fn view(&self) -> ValueView {
        self.selected.view
    }
}
