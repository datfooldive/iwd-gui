#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceInfo {
    pub(crate) name: String,
    pub(crate) path: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VisibleNetwork {
    pub(crate) ssid: String,
    pub(crate) security: String,
    pub(crate) signal: String,
    pub(crate) connected: bool,
    pub(crate) path: String,
    pub(crate) device_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct KnownNetwork {
    pub(crate) name: String,
    pub(crate) network_type: String,
    pub(crate) autoconnect: Option<bool>,
    pub(crate) hidden: Option<bool>,
    pub(crate) path: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ActiveTab {
    #[default]
    Networks,
    Saved,
}
