use rwiki_core::AppConfig;

/// API 层配置
///
/// 直接复用 domain-core 的 AppConfig。
/// 如需扩展 API 层特有的配置项，可在此添加。
pub type ApiConfig = AppConfig;
