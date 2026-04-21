//! Email sender. Thin wrapper around resend-rs.
//! Configuration comes from BC_RESEND_API_KEY and BC_EMAIL_FROM env vars.
//! When BC_RESEND_API_KEY is unset, emails are logged (dev fallback).

use resend_rs::{types::CreateEmailBaseOptions, Resend};
use std::sync::Arc;

#[derive(Clone)]
pub struct Mailer {
    inner: Option<Arc<Resend>>,
    from: String,
}

impl Mailer {
    pub fn new(api_key: Option<String>, from: Option<String>) -> Self {
        let from = from.unwrap_or_else(|| "BoraCall <onboarding@resend.dev>".to_string());
        let inner = api_key.map(|k| Arc::new(Resend::new(&k)));
        Self { inner, from }
    }

    pub async fn send_otp(&self, to: &str, code: &str) -> anyhow::Result<()> {
        let subject = "Seu código do BoraCall";
        let text = format!(
            "Olá,\n\nSeu código de verificação é: {code}\n\n\
             Ele expira em 10 minutos. Se você não pediu isto, ignore este e-mail.\n\n\
             — BoraCall"
        );
        let html = format!(
            r#"<!doctype html><html><body style="font-family:-apple-system,system-ui,sans-serif;background:#14151a;color:#eee;padding:40px 20px">
<div style="max-width:460px;margin:0 auto;background:#1a1c22;padding:32px;border-radius:4px;border:1px solid #333">
<h1 style="margin:0 0 16px;font-size:18px">Seu código de verificação</h1>
<p style="color:#aaa;margin:0 0 24px">Use este código para continuar no BoraCall. Ele expira em 10 minutos.</p>
<div style="font-family:JetBrains Mono,ui-monospace,monospace;font-size:32px;letter-spacing:8px;text-align:center;background:#0e0f13;padding:20px;border:1px solid #333;color:#f5b947">{code}</div>
<p style="color:#666;font-size:12px;margin-top:24px">Se você não solicitou este código, pode ignorar esta mensagem.</p>
</div></body></html>"#
        );

        match &self.inner {
            Some(client) => {
                let email = CreateEmailBaseOptions::new(&self.from, [to], subject)
                    .with_html(&html)
                    .with_text(&text);
                let res = client.emails.send(email).await?;
                tracing::info!(to = %to, id = ?res.id, "otp email sent");
                Ok(())
            }
            None => {
                tracing::warn!(to = %to, code = %code, "BC_RESEND_API_KEY not set — logging OTP instead of sending");
                Ok(())
            }
        }
    }
}
