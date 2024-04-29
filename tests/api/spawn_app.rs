use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::net::TcpListener;
use uuid::Uuid;
use zero2prod2::configuration::get_configuration;

pub struct TestApp {
    /// Address the application is serving HTTP requests from. e.g., localhost:8080.
    pub address: String,
    /// The applications underlying DB.
    pub db_pool: PgPool,
}

/// Launch our application in the background and returns address
pub async fn spawn_app() -> TestApp {
    // Run the app on an ephemral port on the localhost.
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.01:{}", port);

    // Create a fresh DB for this test run.
    let configuration = {
        let mut configuration = get_configuration().expect("failed to get configuration");
        // Use a random name to make tests hermetic.
        configuration.database.database_name = Uuid::new_v4().to_string();
        configuration
    };

    let db_pool = {
        let config = &configuration.database;
        // Create the database
        let mut connection = PgConnection::connect(&config.connection_string_without_db())
            .await
            .expect("failed to connect to postgres");

        connection
            .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
            .await
            .expect("Failed to create ephemeral database.");

        // migrate the database
        let connection_pool = PgPool::connect(&config.connection_string())
            .await
            .expect("Failed to connect to postgres after creating ephemeral db");
        sqlx::migrate!("./migrations")
            .run(&connection_pool)
            .await
            .expect("failed to run DB migration ./migrations");
        connection_pool
    };

    let server =
        zero2prod2::startup::run(listener, db_pool.clone()).expect("failed to bind address");
    let _ = tokio::spawn(server);

    TestApp { address, db_pool }
}
