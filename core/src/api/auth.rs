//! `/passport/*` endpoints: register / login / forgot password / email verify.

use serde::Serialize;

use super::types::AuthResult;
use super::HttpClient;
use crate::error::Result;

/// Captcha payload routed to the field the backend's `CaptchaService`
/// actually reads — these differ per provider and a mismatched key silently
/// fails the challenge with HTTP 400 (this was the original Turnstile/v3
/// bug). The mapping is, verbatim from `app/Services/CaptchaService.php`:
///
/// | `captcha_type`  | wire field           | CaptchaService.php |
/// |-----------------|----------------------|--------------------|
/// | `turnstile`     | `turnstile_token`    | :41                |
/// | `recaptcha-v3`  | `recaptcha_v3_token` | :68                |
/// | `recaptcha`(v2) | `recaptcha_data`     | :98                |
///
/// Exactly one field is ever populated; the rest skip-serialize so the body
/// carries only the single key the panel will look for. An empty payload
/// (no captcha configured / not required) serializes to nothing.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Captcha<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turnstile_token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recaptcha_v3_token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recaptcha_data: Option<&'a str>,
}

impl<'a> Captcha<'a> {
    /// Route a single resolved `token` into the field matching `captcha_type`
    /// (the `guest/comm/config.captcha_type` value). An empty token or an
    /// unrecognised type yields an empty payload — we never guess a field,
    /// because sending the token under the wrong key is exactly the failure
    /// this type exists to prevent.
    pub fn from_type(captcha_type: Option<&str>, token: Option<&'a str>) -> Self {
        let token = match token {
            Some(t) if !t.is_empty() => t,
            _ => return Self::default(),
        };
        match captcha_type {
            Some("turnstile") => Self {
                turnstile_token: Some(token),
                ..Self::default()
            },
            Some("recaptcha-v3") => Self {
                recaptcha_v3_token: Some(token),
                ..Self::default()
            },
            Some("recaptcha") => Self {
                recaptcha_data: Some(token),
                ..Self::default()
            },
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LoginRequest<'a> {
    pub email: &'a str,
    pub password: &'a str,
    #[serde(flatten)]
    pub captcha: Captcha<'a>,
}

#[derive(Debug, Serialize)]
pub struct RegisterRequest<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub email_code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<&'a str>,
    #[serde(flatten)]
    pub captcha: Captcha<'a>,
}

impl HttpClient {
    pub async fn login(&self, req: &LoginRequest<'_>) -> Result<AuthResult> {
        self.post_json("/api/v1/passport/auth/login", req).await
    }

    pub async fn register(&self, req: &RegisterRequest<'_>) -> Result<AuthResult> {
        self.post_json("/api/v1/passport/auth/register", req).await
    }

    /// `sendEmailVerify` is captcha-gated on upstream Xboard (register /
    /// forget flows). The token must arrive under the provider-specific key
    /// just like login/register, so we take a fully-routed [`Captcha`].
    pub async fn send_email_verify(&self, email: &str, captcha: Captcha<'_>) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            #[serde(flatten)]
            captcha: Captcha<'a>,
        }
        // The endpoint returns `{ data: true }`; we don't care about the body.
        let _: serde_json::Value = self
            .post_json(
                "/api/v1/passport/comm/sendEmailVerify",
                &Body { email, captcha },
            )
            .await?;
        Ok(())
    }

    pub async fn forget_password(
        &self,
        email: &str,
        password: &str,
        email_code: &str,
        captcha: Captcha<'_>,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            password: &'a str,
            email_code: &'a str,
            #[serde(flatten)]
            captcha: Captcha<'a>,
        }
        let _: serde_json::Value = self
            .post_json(
                "/api/v1/passport/auth/forget",
                &Body {
                    email,
                    password,
                    email_code,
                    captcha,
                },
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captcha_routes_to_correct_backend_field() {
        let ts = Captcha::from_type(Some("turnstile"), Some("tok"));
        assert_eq!(
            serde_json::to_value(ts).unwrap(),
            serde_json::json!({ "turnstile_token": "tok" })
        );

        let v3 = Captcha::from_type(Some("recaptcha-v3"), Some("tok"));
        assert_eq!(
            serde_json::to_value(v3).unwrap(),
            serde_json::json!({ "recaptcha_v3_token": "tok" })
        );

        let v2 = Captcha::from_type(Some("recaptcha"), Some("tok"));
        assert_eq!(
            serde_json::to_value(v2).unwrap(),
            serde_json::json!({ "recaptcha_data": "tok" })
        );
    }

    #[test]
    fn captcha_empty_when_no_token_or_unknown_type() {
        assert_eq!(
            serde_json::to_value(Captcha::from_type(Some("turnstile"), None)).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(Captcha::from_type(Some("turnstile"), Some(""))).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(Captcha::from_type(Some("hcaptcha"), Some("tok"))).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(Captcha::from_type(None, Some("tok"))).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn login_request_flattens_captcha_inline() {
        let req = LoginRequest {
            email: "a@b.c",
            password: "pw",
            captcha: Captcha::from_type(Some("turnstile"), Some("tok")),
        };
        assert_eq!(
            serde_json::to_value(&req).unwrap(),
            serde_json::json!({
                "email": "a@b.c",
                "password": "pw",
                "turnstile_token": "tok"
            })
        );
    }
}
