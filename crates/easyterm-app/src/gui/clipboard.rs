use arboard::Clipboard;

pub(crate) struct ClipboardState {
    clipboard: Option<Clipboard>,
}

impl ClipboardState {
    pub(crate) fn new() -> Self {
        Self {
            clipboard: Clipboard::new().ok(),
        }
    }

    pub(crate) fn set_text(&mut self, text: &str) -> Result<(), arboard::Error> {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Ok(());
        };

        clipboard.set_text(text.to_string())
    }

    pub(crate) fn get_text(&mut self) -> Result<Option<String>, arboard::Error> {
        let Some(clipboard) = self.clipboard.as_mut() else {
            return Ok(None);
        };

        clipboard.get_text().map(Some)
    }
}
