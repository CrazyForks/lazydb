use crate::{help::HelpState, model::omni::OmniState};

#[derive(Debug, Default)]
pub struct HelpPanelSession {
    pub origin_overlay: Option<crate::model::workspace::Overlay>,
    pub help: Option<HelpState>,
    pub omni: Option<OmniState>,
}
