//! Build-time constants. The backend URL is intentionally hardcoded here so
//! end users never have to think about server addresses — they install the
//! app, enter email + password, done. To rebrand for a different deployment
//! flip the const and rebuild.

/// Xboard backend API root. Must be the bare host (no path, no fragment) —
/// `HttpClient::endpoint()` joins absolute paths like `/api/v1/...` and the
/// resulting URL takes only this host's scheme + authority.
pub const BACKEND_URL: &str = "https://www.isufe.me";

/// Default UI locale on cold start. Matches the V1 `Accept-Language` header
/// the backend uses to pick error message translations.
pub const DEFAULT_LOCALE: &str = "zh-CN";
