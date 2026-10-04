//! Transactional account mail. The recipient and token are never logged.
use crate::auth::Secret;
use serde_json::{Value, json};
use std::{fmt, future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub enum EmailLocale {
    En,
    De,
}
impl EmailLocale {
    pub(crate) fn from_tag(tag: Option<&str>) -> Self {
        if tag.is_some_and(|t| t == "de" || t.starts_with("de-")) {
            Self::De
        } else {
            Self::En
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EmailErrorCode {
    Unverified,
    InvalidToken,
    Unavailable,
}
impl EmailErrorCode {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Unverified => "email_unverified",
            Self::InvalidToken => "invalid_token",
            Self::Unavailable => "mail_unavailable",
        }
    }
}

pub(crate) fn message(code: &str, locale: EmailLocale) -> Value {
    let german = matches!(locale, EmailLocale::De);
    let fallback = match (code, german) {
        ("verification_sent", false) => {
            "If this address needs verification, you will receive an email with the next step."
        }
        ("verification_sent", true) => {
            "Falls diese Adresse bestätigt werden muss, erhältst du eine E-Mail mit dem nächsten Schritt."
        }
        ("password_reset_sent", false) => "If an account uses this address, you will receive a password reset email.",
        ("password_reset_sent", true) => {
            "Falls ein Konto diese Adresse verwendet, erhältst du eine E-Mail zum Zurücksetzen des Passworts."
        }
        ("email_verified", false) => "Email verified. You can now sign in.",
        ("email_verified", true) => "E-Mail bestätigt. Du kannst dich jetzt anmelden.",
        ("password_reset_complete", false) => "Password reset. Sign in with your new password.",
        ("password_reset_complete", true) => "Passwort zurückgesetzt. Melde dich mit deinem neuen Passwort an.",
        ("email_unverified", false) => "Verify your email before signing in.",
        ("email_unverified", true) => "Bestätige deine E-Mail, bevor du dich anmeldest.",
        ("invalid_token", false) => "This link is invalid, expired or already used. Request a new link.",
        ("invalid_token", true) => {
            "Dieser Link ist ungültig, abgelaufen oder bereits verwendet. Fordere einen neuen Link an."
        }
        (_, false) => "Account email is unavailable. Please try again later.",
        (_, true) => "Konto-E-Mails sind derzeit nicht verfügbar. Versuche es später erneut.",
    };
    json!({"key":format!("account.email.{code}"),"params":{},"fallback":fallback})
}

pub(crate) struct OutboundEmail {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    pub idempotency_key: String,
}
impl fmt::Debug for OutboundEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OutboundEmail(***)")
    }
}
pub(crate) trait Delivery: Send + Sync {
    fn send(&self, email: OutboundEmail) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

#[derive(Clone)]
pub struct EmailConfig {
    pub(crate) origin: String,
    pub(crate) delivery: Arc<dyn Delivery>,
    pub(crate) work: Arc<tokio::sync::Semaphore>,
}
impl fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmailConfig")
            .field("origin", &self.origin)
            .field("delivery", &"redacted")
            .finish()
    }
}
impl EmailConfig {
    /// No key, malformed configuration or an untrusted URL disables new email actions.
    /// Does not make a request, inspect a domain or send mail during startup.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("RESEND_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty() && !v.chars().any(char::is_whitespace))?;
        let sender = std::env::var("ATLAS_ACCOUNT_EMAIL_FROM").or_else(|_| std::env::var("RESEND_FROM")).ok()?;
        if sender.len() > 320 || sender.chars().any(char::is_control) {
            return None;
        }
        let address = sender
            .rsplit_once('<')
            .map(|(_, a)| a.trim_end_matches('>'))
            .unwrap_or(&sender);
        crate::model::clean_email(address).ok()?;
        let mut origin = reqwest::Url::parse(&std::env::var("ATLAS_ACCOUNT_PUBLIC_ORIGIN").ok()?).ok()?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !matches!(origin.path(), "" | "/")
        {
            return None;
        }
        origin.set_path("");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .ok()?;
        Some(Self {
            origin: origin.to_string().trim_end_matches('/').to_owned(),
            delivery: Arc::new(Resend {
                client,
                key: Secret::new(key),
                sender,
            }),
            work: {
                static WORK: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
                WORK.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4))).clone()
            },
        })
    }

    pub(crate) fn prepare(
        &self,
        to: String,
        purpose: &str,
        token: &Secret,
        hash: &str,
        locale: EmailLocale,
        minutes: u64,
    ) -> OutboundEmail {
        let flow = if purpose == "verify_email" {
            "verify-email"
        } else {
            "reset-password"
        };
        let lang = if matches!(locale, EmailLocale::De) { "de" } else { "en" };
        let link = format!(
            "{}/zebra/account?flow={flow}&lang={lang}#token={}",
            self.origin,
            token.expose()
        );
        let (subject, body) = match (purpose, matches!(locale, EmailLocale::De)) {
            ("verify_email", false) => (
                "Verify your Zebratlas email",
                format!(
                    "Confirm your email to finish creating your Zebratlas account. This link expires in {minutes} minutes.\n\n{link}\n\nIf you did not request this, ignore this email."
                ),
            ),
            ("verify_email", true) => (
                "Bestätige deine Zebratlas-E-Mail",
                format!(
                    "Bestätige deine E-Mail, um dein Zebratlas-Konto anzulegen. Dieser Link läuft in {minutes} Minuten ab.\n\n{link}\n\nFalls du dies nicht angefordert hast, ignoriere diese E-Mail."
                ),
            ),
            (_, false) => (
                "Reset your Zebratlas password",
                format!(
                    "Choose a new password for your Zebratlas account. This link expires in {minutes} minutes.\n\n{link}\n\nIf you did not request this, ignore this email. Your password has not changed."
                ),
            ),
            (_, true) => (
                "Zebratlas-Passwort zurücksetzen",
                format!(
                    "Wähle ein neues Passwort für dein Zebratlas-Konto. Dieser Link läuft in {minutes} Minuten ab.\n\n{link}\n\nFalls du dies nicht angefordert hast, ignoriere diese E-Mail. Dein Passwort wurde nicht geändert."
                ),
            ),
        };
        let html = format!(
            "<div>{}</div>",
            body.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\n', "<br>")
        )
        .replace(
            &link.replace('&', "&amp;"),
            &format!(
                "<a href=\"{}\">{}</a>",
                link.replace('&', "&amp;"),
                link.replace('&', "&amp;")
            ),
        );
        OutboundEmail {
            to,
            subject: subject.into(),
            text: body,
            html,
            idempotency_key: format!("account-{purpose}-{hash}"),
        }
    }
}

struct Resend {
    client: reqwest::Client,
    key: Secret,
    sender: String,
}
impl Resend {
    fn request(&self, email: OutboundEmail) -> reqwest::RequestBuilder {
        self.client.post("https://api.resend.com/emails").bearer_auth(self.key.expose()).header("Idempotency-Key",email.idempotency_key)
            .json(&json!({"from":self.sender,"to":[email.to],"subject":email.subject,"text":email.text,"html":email.html}))
    }
}
impl Delivery for Resend {
    fn send(&self, email: OutboundEmail) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            let response = self.request(email).send().await;
            // Provider error bodies may contain recipients or links. Never read or log them.
            response.is_ok_and(|r| r.status().is_success())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resend_request_contract_is_fixed_and_secrets_are_redacted_without_sending() {
        let resend = Resend {
            client: reqwest::Client::new(),
            key: Secret::new("fixture-key-not-real"),
            sender: "Zebratlas <noreply@auth.zebratlas.org>".into(),
        };
        let request = resend
            .request(OutboundEmail {
                to: "fixture@example.invalid".into(),
                subject: "Fixture".into(),
                text: "Fixture text".into(),
                html: "<p>Fixture</p>".into(),
                idempotency_key: "fixture-idempotency".into(),
            })
            .build()
            .unwrap();
        assert_eq!(request.url().as_str(), "https://api.resend.com/emails");
        assert_eq!(request.headers()["Idempotency-Key"], "fixture-idempotency");
        assert_eq!(request.headers()["Authorization"], "Bearer fixture-key-not-real");
        let body: Value = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["from"], "Zebratlas <noreply@auth.zebratlas.org>");
        assert_eq!(body["to"], json!(["fixture@example.invalid"]));
        assert!(body.get("open_tracking").is_none());
        assert!(body.get("click_tracking").is_none());
        assert_eq!(format!("{:?}", resend.key), "Secret(***)");
    }
}
