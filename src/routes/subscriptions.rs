/// /subscriptions handlers.
use actix_web::{web, HttpResponse};
use chrono::Utc;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{NewSubscriber, SubscriberEmail, SubscriberName},
    email_client::EmailClient,
    startup::ApplicationBaseUrl,
};

#[derive(serde::Deserialize)]
pub struct FormData {
    pub email: String,
    pub name: String,
}

/// Subscribes a user to the newsletter
///  - Preconditions
///     * email and name set.
#[tracing::instrument(
    name = "Adding a new subscriber",
    skip(form, db_pool, base_url),
    fields(
        subscriber_email = %form.email,
        subscriber_name = %form.name
    )
)]
pub async fn subscribe(
    form: web::Form<FormData>,
    db_pool: web::Data<PgPool>,
    email_client: web::Data<EmailClient>,
    base_url: web::Data<ApplicationBaseUrl>,
) -> HttpResponse {
    let new_subscriber = match form.0.try_into() {
        Ok(new_subscriber) => new_subscriber,
        Err(_) => return HttpResponse::BadRequest().finish(),
    };

    let subscriber_id = match insert_subscriber(&new_subscriber, &db_pool).await {
        Ok(subscriber_id) => subscriber_id,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let subscription_token = generate_subscription_token();

    if store_token(&db_pool, subscriber_id, &subscription_token)
        .await
        .is_err()
    {
        return HttpResponse::InternalServerError().finish();
    }

    if send_confirmation_email(
        &email_client,
        new_subscriber,
        &base_url.0,
        &subscription_token,
    )
    .await
    .is_err()
    {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().finish()
}

#[tracing::instrument(
    name = "Send a confirmation email to a new subscriber",
    skip(subscription_token, pool)
)]
async fn store_token(
    pool: &PgPool,
    subscriber_id: Uuid,
    subscription_token: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO subscription_tokens (subscription_token, subscriber_id)
    VALUES ($1, $2)"#,
        subscription_token,
        subscriber_id
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to store token query: {:?}", e);
        e
    })?;
    Ok(())
}

#[tracing::instrument(
    name = "Send a confirmation email to a new subscriber",
    skip(email_client, new_subscriber, base_url, token)
)]
async fn send_confirmation_email(
    email_client: &EmailClient,
    new_subscriber: NewSubscriber,
    base_url: &str,
    token: &str,
) -> Result<(), reqwest::Error> {
    let confirmation_link = format!(
        "{}/subscriptions/confirm?subscription_token={}",
        base_url, token
    );
    email_client
        .send_email(
            new_subscriber.email,
            "Welcome!",
            &format!(
                "Welcome to our newsletter! <br/> \
                Click <a href=\"{}\">here</a> to confirm your subscription.",
                confirmation_link
            ),
            &format!(
                "Welcome to our newsletter!\n Visit {} to confirm your subscription.",
                confirmation_link
            ),
        )
        .await
}

impl TryFrom<FormData> for NewSubscriber {
    type Error = String;

    fn try_from(value: FormData) -> Result<Self, Self::Error> {
        let name = SubscriberName::parse(value.name)?;
        let email = SubscriberEmail::parse(value.email)?;
        Ok(Self { email, name })
    }
}

#[tracing::instrument(
    name = "Saving new subscriber to database.",
    skip(new_subscriber, pool)
)]
async fn insert_subscriber(
    new_subscriber: &NewSubscriber,
    pool: &PgPool,
) -> Result<Uuid, sqlx::Error> {
    let subscriber_id = Uuid::new_v4();
    sqlx::query!(
        r#"
       INSERT INTO subscriptions (id, email, name, subscribed_at, status)
       VALUES($1, $2, $3, $4, 'pending_confirmation')
       "#,
        subscriber_id,
        new_subscriber.email.as_ref(),
        new_subscriber.name.as_ref(),
        Utc::now()
    )
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("failed to exec query: {:?}", e);
        e
    })?;
    Ok(subscriber_id)
}

/// Generate a random 25-character-long case senstive subscription token.
fn generate_subscription_token() -> String {
    let mut rng = thread_rng();
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(25)
        .collect()
}
