use axum::{async_trait, extract::{FromRef, FromRequestParts, State}, http::{request::Parts, StatusCode}, response::Json, routing::{get, post}, Router, debug_handler};
use diesel::prelude::*;
use diesel_async::{
    pooled_connection::AsyncDieselConnectionManager, AsyncPgConnection, RunQueryDsl,
};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// normally part of your generated schema.rs file
table! {
    users (id) {
        id -> Integer,
        name -> Text,
        hair_color -> Nullable<Text>,
    }
}

#[derive(serde::Serialize, Selectable, Queryable)]
struct User {
    id: i32,
    name: String,
    hair_color: Option<String>,
}

#[derive(serde::Deserialize, Insertable)]
#[diesel(table_name = users)]
struct NewUser {
    name: String,
    hair_color: Option<String>,
}

pub type DB = diesel::pg::Pg;
pub type DbPoolConn =
bb8::PooledConnection<'static, AsyncDieselConnectionManager<AsyncPgConnection>>;
pub type DbPool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;


pub fn internal_error<E>(err: E) -> (StatusCode, String)
    where
        E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}


pub struct DatabaseConnection(pub DbPoolConn);

#[async_trait]
impl<S> FromRequestParts<S> for DatabaseConnection
    where
        S: Send + Sync,
        DbPool: FromRef<S>,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = DbPool::from_ref(state);

        Ok(Self((pool.get_owned().await.map_err(internal_error)?)))
    }
}


struct AppState{
    pool: DbPool,
    meilisearch_client: meilisearch_sdk::client::Client,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_diesel_async_postgres=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_url = std::env::var("DATABASE_URL").unwrap();

    // set up connection pool
    let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
    let pool = bb8::Pool::builder().build(config).await.unwrap();

    let meilisearch_client = meilisearch_sdk::Client::new("http://localhost:7700", Some("a"));

    // build our application with some routes
    let app = Router::new()
        .route("/user/create", post(create_user))
        .with_state(AppState{pool, meilisearch_client});

    // run it with hyper
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[debug_handler(state = AppState)]
async fn create_user(
    // State(appstate): State<AppState>,
    // State(pool): State<DbPool>,
    State(DatabaseConnection(mut conn)): State<DatabaseConnection>,
    State(meilisearch_client): State<meilisearch_sdk::client::Client>,
    Json(new_user): Json<NewUser>,
) -> Result<Json<User>, (StatusCode, String)> {
    // let mut conn = appstate.pool.get().await.unwrap();
    // let mut conn = pool.get_owned().await.unwrap();


    let res = diesel::insert_into(users::table)
        .values(new_user)
        .returning(User::as_returning())
        .get_result(&mut conn)
        .await
        .unwrap();
    Ok(Json(res))
}
