//! `/passport/*` endpoints: register / login / forgot password / email verify.

use serde::Serialize;

use super::types::AuthResult;
use super::HttpClient;
use crate::error::Result;

#[derive(Debug, Serialize)]
pub struct LoginRequest<'a> {
    pub email: &'a str,
    pub password: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recaptcha_data: Option<&'a str>,
    #[serde(
        rename = "cf-turnstile-response",
        skip_serializing_if = "Option::is_none"
    )]
    pub turnstile: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub struct RegisterRequest<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub email_code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recaptcha_data: Option<&'a str>,
    #[serde(
        rename = "cf-turnstile-response",
        skip_serializing_if = "Option::is_none"
    )]
    pub turnstile: Option<&'a str>,
}

impl HttpClient {
    pub async fn login(&self, req: &LoginRequest<'_>) -> Result<AuthResult> {
        self.post_json("/api/v1/passport/auth/login", req).await
    }

    pub async fn register(&self, req: &RegisterRequest<'_>) -> Result<AuthResult> {
        self.post_json("/api/v1/passport/auth/register", req).await
    }

    pub async fn send_email_verify(&self, email: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
        }
        // The endpoint returns `{ data: true }`; we don't care about the body.
        let _: serde_json::Value = self
            .post_json("/api/v1/passport/comm/sendEmailVerify", &Body { email })
            .await?;
        Ok(())
    }

    pub async fn forget_password(
        &self,
        email: &str,
        password: &str,
        email_code: &str,
        recaptcha: Option<&str>,
        turnstile: Option<&str>,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            password: &'a str,
            email_code: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            recaptcha_data: Option<&'a str>,
            #[serde(
                rename = "cf-turnstile-response",
                skip_serializing_if = "Option::is_none"
            )]
            turnstile: Option<&'a str>,
        }
        let _: serde_json::Value = self
            .post_json(
                "/api/v1/passport/auth/forget",
                &Body {
                    email,
                    password,
                    email_code,
                    recaptcha_data: recaptcha,
                    turnstile,
                },
            )
            .await?;
        Ok(())
    }
}
