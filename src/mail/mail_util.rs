use lettre::smtp::authentication::{Credentials, Mechanism};
use lettre::smtp::extension::ClientId;
use lettre::smtp::ConnectionReuseParameters;
use lettre::{SmtpClient, Transport};
use lettre_email::{mime::TEXT_PLAIN, Email};
use std::path::Path;
use crate::data_shape::MailConf;

pub fn send_test_mail(mail_conf: &MailConf, mail_to: impl AsRef<str>) -> Result<(), failure::Error> {
    send_text_mail(mail_conf, mail_to, "this is a test mail from bk-over-ssh.", "Hello world.")
}

pub fn send_text_mail(mail_conf: &MailConf, mail_to: impl AsRef<str>, subject: impl AsRef<str>, text: impl AsRef<str>) -> Result<(), failure::Error> {
    let email = Email::builder()
        // Addresses can be specified by the tuple (email, alias)
        .to((mail_to.as_ref(), ""))
        // ... or by an address only
        .from(mail_conf.from.as_str())
        .subject(subject.as_ref())
        .text(text.as_ref())
        .attachment_from_file(Path::new("Cargo.toml"), None, &TEXT_PLAIN)
        .unwrap()
        .build()
        .unwrap();

    // Open a local connection on port 25
    // let mut mailer = SmtpClient::new_unencrypted_localhost().unwrap().transport();
    let mut mailer = SmtpClient::new_simple(mail_conf.hostname.as_str())?
        // Set the name sent during EHLO/HELO, default is `localhost`
        // .hello_name(ClientId::Domain("my.hostname.tld".to_string()))
        // Add credentials for authentication
        .credentials(Credentials::new(
            mail_conf.username.clone(),
            mail_conf.password.clone(),
        ))
        // Enable SMTPUTF8 if the server supports it
        .smtp_utf8(true)
        // Configure expected authentication mechanism
        .authentication_mechanism(Mechanism::Plain)
        // Enable connection reuse
        .connection_reuse(ConnectionReuseParameters::ReuseUnlimited)
        .transport();

    // Send the email
    mailer.send(email.into())?;

    Ok(())
}
