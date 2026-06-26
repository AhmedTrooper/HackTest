use axum::{
    extract::{FromRequest, Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum::extract::Multipart;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

// SQLx and AWS S3 imports
use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use sqlx::Row;

const MAX_UPLOAD_SIZE: usize = 10 * 1024 * 1024; // 10 MB

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Submission {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub category: Category,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub file_info: Option<UploadedFile>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    #[serde(rename = "Bug Report")]
    BugReport,
    #[serde(rename = "Feature Request")]
    FeatureRequest,
    Feedback,
    Other,
}

impl Category {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Bug Report" => Some(Self::BugReport),
            "Feature Request" => Some(Self::FeatureRequest),
            "Feedback" => Some(Self::Feedback),
            "Other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "Low" => Some(Self::Low),
            "Medium" => Some(Self::Medium),
            "High" => Some(Self::High),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UploadedFile {
    pub file_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct UploadMetadata {
    pub file_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub size_bytes: usize,
    pub storage_path: String,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Option<sqlx::PgPool>,
    pub s3_client: Option<aws_sdk_s3::Client>,
    pub s3_bucket: String,
    pub submissions_mem: Arc<RwLock<Vec<Submission>>>,
    pub uploads_mem: Arc<RwLock<HashMap<Uuid, UploadMetadata>>>,
}

#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Vec<ValidationError>,
}

// Custom wrapper to catch and format Axum's built-in JSON deserialization/rejection errors
pub struct AppJson<T>(pub T);

impl<T, S> FromRequest<S> for AppJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => {
                let status = rejection.status();
                let error_message = rejection.body_text();
                let err_res = ErrorResponse {
                    error: "Malformed JSON request".to_string(),
                    details: vec![ValidationError {
                        field: "body".to_string(),
                        message: error_message,
                    }],
                };
                Err((status, Json(err_res)).into_response())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SubmissionRequest {
    pub title: Option<serde_json::Value>,
    pub description: Option<serde_json::Value>,
    pub category: Option<serde_json::Value>,
    pub priority: Option<serde_json::Value>,
    pub tags: Option<serde_json::Value>,
    pub file_id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubmissionListResponse {
    pub items: Vec<Submission>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

#[tokio::main]
async fn main() {
    // Load environment variables from .env file (traverses up parent directories)
    dotenvy::dotenv().ok();

    // Set up logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .init();

    // Create uploads directory (local disk fallback)
    tokio::fs::create_dir_all("./uploads")
        .await
        .expect("Failed to create uploads directory");

    // 1. Initialize PostgreSQL (if DATABASE_URL is set)
    let db_url = std::env::var("DATABASE_URL").ok();
    let db = if let Some(url) = db_url {
        tracing::info!("Connecting to PostgreSQL Database...");
        match sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(3))
            .connect(&url)
            .await
        {
            Ok(pool) => {
                // Initialize database tables on startup
                sqlx::query(
                    r#"
                    CREATE TABLE IF NOT EXISTS file_uploads (
                        file_id UUID PRIMARY KEY,
                        file_name VARCHAR(255) NOT NULL,
                        mime_type VARCHAR(255) NOT NULL,
                        size_bytes BIGINT NOT NULL,
                        storage_path VARCHAR(255) NOT NULL,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                    );
                    "#
                )
                .execute(&pool)
                .await
                .expect("Failed to initialize file_uploads table");

                sqlx::query(
                    r#"
                    CREATE TABLE IF NOT EXISTS submissions (
                        id UUID PRIMARY KEY,
                        title VARCHAR(255) NOT NULL,
                        description TEXT NOT NULL,
                        category VARCHAR(100) NOT NULL,
                        priority VARCHAR(50) NOT NULL,
                        tags TEXT[] NOT NULL,
                        file_id UUID REFERENCES file_uploads(file_id),
                        file_name VARCHAR(255),
                        file_mime_type VARCHAR(255),
                        file_size_bytes BIGINT,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                    );
                    "#
                )
                .execute(&pool)
                .await
                .expect("Failed to initialize submissions table");

                tracing::info!("PostgreSQL connection live and tables initialized!");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("PostgreSQL connection failed (falling back to memory): {}", e);
                None
            }
        }
    } else {
        tracing::info!("No DATABASE_URL set. Storing submissions in-memory.");
        None
    };

    // 2. Initialize S3 / MinIO (if credentials are set)
    let s3_endpoint = std::env::var("S3_ENDPOINT").ok();
    let s3_access_key = std::env::var("S3_ACCESS_KEY").ok();
    let s3_secret_key = std::env::var("S3_SECRET_KEY").ok();
    let s3_bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "hack-bucket".to_string());

    let s3_client = if let (Some(endpoint), Some(access_key), Some(secret_key)) = (s3_endpoint, s3_access_key, s3_secret_key) {
        tracing::info!("Connecting to S3/MinIO bucket [{}] at endpoint [{}]...", s3_bucket, endpoint);
        
        let credentials = aws_sdk_s3::config::Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "env",
        );
        
        let config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(aws_config::Region::new("us-east-1"))
            .endpoint_url(endpoint)
            .load()
            .await;
            
        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .build();
            
        Some(aws_sdk_s3::Client::from_conf(s3_config))
    } else {
        tracing::info!("S3/MinIO credentials missing. Saving uploads to local disk.");
        None
    };

    // Initialize state
    let state = AppState {
        db,
        s3_client,
        s3_bucket,
        submissions_mem: Arc::new(RwLock::new(Vec::new())),
        uploads_mem: Arc::new(RwLock::new(HashMap::new())),
    };

    // Configure CORS
    let allowed_origins_env = std::env::var("ALLOWED_ORIGINS").ok();
    let cors = if let Some(origins_str) = allowed_origins_env {
        let trimmed = origins_str.trim();
        if trimmed.is_empty() {
            CorsLayer::permissive()
        } else {
            let mut origins = Vec::new();
            for origin in trimmed.split(',') {
                let trimmed_origin = origin.trim();
                if !trimmed_origin.is_empty() {
                    if let Ok(value) = trimmed_origin.parse::<axum::http::HeaderValue>() {
                        origins.push(value);
                    }
                }
            }
            if origins.is_empty() {
                CorsLayer::permissive()
            } else {
                CorsLayer::new()
                    .allow_origin(origins)
                    .allow_methods(Any)
                    .allow_headers(Any)
            }
        }
    } else {
        CorsLayer::permissive()
    };

    // Build routes
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/upload", post(upload_file))
        .route("/api/files/{id}", get(get_file))
        .route("/api/submissions", post(create_submission).get(list_submissions))
        .with_state(state)
        .layer(cors);

    // Determine port and bind
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("Server listening on http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}

// GET /health
async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let mut db_status = "unconfigured";
    if let Some(ref pool) = state.db {
        db_status = match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => "ok",
            Err(_) => "error",
        };
    }

    let mut s3_status = "unconfigured";
    if let Some(ref s3) = state.s3_client {
        s3_status = match s3.list_buckets().send().await {
            Ok(_) => "ok",
            Err(_) => "error",
        };
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "postgres": db_status,
            "s3_minio": s3_status
        })),
    )
}

// POST /api/upload
async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, Response> {
    let mut file_data = Vec::new();
    let mut file_name = "unknown.bin".to_string();
    let mut content_type = "application/octet-stream".to_string();

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Multipart error".to_string(),
                details: vec![ValidationError {
                    field: "file".to_string(),
                    message: e.to_string(),
                }],
            }),
        )
            .into_response()
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            file_name = field.file_name().unwrap_or("file.bin").to_string();
            content_type = field.content_type().unwrap_or("application/octet-stream").to_string();

            // Read chunks, checking size limit
            let mut stream = field;
            while let Some(chunk) = stream.chunk().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Error reading stream".to_string(),
                        details: vec![ValidationError {
                            field: "file".to_string(),
                            message: e.to_string(),
                        }],
                    }),
                )
                    .into_response()
            })? {
                if file_data.len() + chunk.len() > MAX_UPLOAD_SIZE {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(ErrorResponse {
                            error: "File too large".to_string(),
                            details: vec![ValidationError {
                                field: "file".to_string(),
                                message: format!("File size exceeds limit of {} MB", MAX_UPLOAD_SIZE / (1024 * 1024)),
                            }],
                        }),
                    )
                        .into_response());
                }
                file_data.extend_from_slice(&chunk);
            }
        }
    }

    if file_data.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "No file provided".to_string(),
                details: vec![ValidationError {
                    field: "file".to_string(),
                    message: "File payload is empty".to_string(),
                }],
            }),
        )
            .into_response());
    }

    let file_id = Uuid::new_v4();
    let storage_filename = format!("{}_{}", file_id, file_name);
    let storage_path = format!("./uploads/{}", storage_filename);
    let file_size = file_data.len();
    let meta = UploadMetadata {
        file_id,
        file_name: file_name.clone(),
        mime_type: content_type.clone(),
        size_bytes: file_size,
        storage_path: storage_filename.clone(),
    };

    if let Some(ref s3) = state.s3_client {
        let body = ByteStream::from(file_data.clone());
        s3.put_object()
            .bucket(&state.s3_bucket)
            .key(&storage_filename)
            .body(body)
            .content_type(&content_type)
            .send()
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "S3 upload failed".to_string(),
                        details: vec![ValidationError {
                            field: "s3".to_string(),
                            message: e.to_string(),
                        }],
                    }),
                )
                    .into_response()
            })?;
    } else {
        tokio::fs::write(&storage_path, &file_data).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to save file locally".to_string(),
                    details: vec![ValidationError {
                        field: "file".to_string(),
                        message: e.to_string(),
                    }],
                }),
            )
                .into_response()
        })?;
    }

    // Save metadata in postgres if pool is initialized
    if let Some(ref pool) = state.db {
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO file_uploads (file_id, file_name, mime_type, size_bytes, storage_path)
            VALUES ($1, $2, $3, $4, $5)
            "#
        )
        .bind(file_id)
        .bind(&file_name)
        .bind(&content_type)
        .bind(file_size as i64)
        .bind(&meta.storage_path)
        .execute(pool)
        .await
        {
            tracing::error!("Failed to persist file upload metadata in Postgres: {}", e);
        }
    }

    state.uploads_mem.write().await.insert(file_id, meta);

    Ok((
        StatusCode::OK,
        Json(UploadedFile {
            file_id,
            file_name,
            mime_type: content_type,
            size_bytes: file_size,
        }),
    ))
}

// GET /api/files/{id}
async fn get_file(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> impl IntoResponse {
    let file_id = match Uuid::parse_str(&id_str) {
        Ok(uid) => uid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid file ID format".to_string(),
                    details: vec![ValidationError {
                        field: "id".to_string(),
                        message: "Must be a valid UUID".to_string(),
                    }],
                }),
            )
                .into_response();
        }
    };

    // Retrieve file metadata from Postgres first, falling back to memory
    let meta = {
        let mut resolved = None;
        if let Some(ref pool) = state.db {
            if let Ok(Some(row)) = sqlx::query(
                "SELECT file_id, file_name, mime_type, size_bytes, storage_path FROM file_uploads WHERE file_id = $1"
            )
            .bind(file_id)
            .fetch_optional(pool)
            .await
            {
                resolved = Some(UploadMetadata {
                    file_id: row.get("file_id"),
                    file_name: row.get("file_name"),
                    mime_type: row.get("mime_type"),
                    size_bytes: row.get::<i64, _>("size_bytes") as usize,
                    storage_path: row.get("storage_path"),
                });
            }
        }
        if resolved.is_none() {
            resolved = state.uploads_mem.read().await.get(&file_id).cloned();
        }
        resolved
    };

    if let Some(meta) = meta {
        // Stream from S3 or read from Disk
        if let Some(ref s3) = state.s3_client {
            match s3.get_object()
                .bucket(&state.s3_bucket)
                .key(&meta.storage_path)
                .send()
                .await
            {
                Ok(output) => {
                    match output.body.collect().await {
                        Ok(collected) => {
                            (
                                StatusCode::OK,
                                [(axum::http::header::CONTENT_TYPE, meta.mime_type.clone())],
                                collected.into_bytes(),
                            )
                                .into_response()
                        }
                        Err(e) => {
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorResponse {
                                    error: "Failed to read S3 object stream".to_string(),
                                    details: vec![ValidationError {
                                        field: "s3".to_string(),
                                        message: e.to_string(),
                                    }],
                                }),
                            )
                                .into_response()
                        }
                    }
                }
                Err(e) => {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "S3 download request failed".to_string(),
                            details: vec![ValidationError {
                                field: "s3".to_string(),
                                message: e.to_string(),
                            }],
                        }),
                    )
                        .into_response()
                }
            }
        } else {
            let local_path = format!("./uploads/{}", meta.storage_path);
            match tokio::fs::read(&local_path).await {
                Ok(bytes) => {
                    (
                        StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, meta.mime_type.clone())],
                        bytes,
                    )
                        .into_response()
                }
                Err(e) => {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "File read error".to_string(),
                            details: vec![ValidationError {
                                field: "disk".to_string(),
                                message: e.to_string(),
                            }],
                        }),
                    )
                        .into_response()
                }
            }
        }
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "File not found".to_string(),
                details: vec![ValidationError {
                    field: "id".to_string(),
                    message: "File metadata not found".to_string(),
                }],
            }),
        )
            .into_response()
    }
}

// POST /api/submissions
async fn create_submission(
    State(state): State<AppState>,
    AppJson(payload): AppJson<SubmissionRequest>,
) -> Result<impl IntoResponse, Response> {
    let mut details = Vec::new();

    // 1. Validate title
    let title = match payload.title {
        None => {
            details.push(ValidationError {
                field: "title".to_string(),
                message: "title is missing".to_string(),
            });
            None
        }
        Some(serde_json::Value::Null) => {
            details.push(ValidationError {
                field: "title".to_string(),
                message: "title cannot be null".to_string(),
            });
            None
        }
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                details.push(ValidationError {
                    field: "title".to_string(),
                    message: "title cannot be empty".to_string(),
                });
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(_) => {
            details.push(ValidationError {
                field: "title".to_string(),
                message: "title must be a string".to_string(),
            });
            None
        }
    };

    // 2. Validate description
    let description = match payload.description {
        None => {
            details.push(ValidationError {
                field: "description".to_string(),
                message: "description is missing".to_string(),
            });
            None
        }
        Some(serde_json::Value::Null) => {
            details.push(ValidationError {
                field: "description".to_string(),
                message: "description cannot be null".to_string(),
            });
            None
        }
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                details.push(ValidationError {
                    field: "description".to_string(),
                    message: "description cannot be empty".to_string(),
                });
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(_) => {
            details.push(ValidationError {
                field: "description".to_string(),
                message: "description must be a string".to_string(),
            });
            None
        }
    };

    // 3. Validate category (strict case-sensitive match)
    let category = match payload.category {
        None => {
            details.push(ValidationError {
                field: "category".to_string(),
                message: "category is missing".to_string(),
            });
            None
        }
        Some(serde_json::Value::Null) => {
            details.push(ValidationError {
                field: "category".to_string(),
                message: "category cannot be null".to_string(),
            });
            None
        }
        Some(serde_json::Value::String(s)) => {
            match Category::from_str(&s) {
                Some(cat) => Some(cat),
                None => {
                    details.push(ValidationError {
                        field: "category".to_string(),
                        message: "category must be exactly 'Bug Report', 'Feature Request', 'Feedback', or 'Other'".to_string(),
                    });
                    None
                }
            }
        }
        Some(_) => {
            details.push(ValidationError {
                field: "category".to_string(),
                message: "category must be a string".to_string(),
            });
            None
        }
    };

    // 4. Validate priority (strict case-sensitive match)
    let priority = match payload.priority {
        None => {
            details.push(ValidationError {
                field: "priority".to_string(),
                message: "priority is missing".to_string(),
            });
            None
        }
        Some(serde_json::Value::Null) => {
            details.push(ValidationError {
                field: "priority".to_string(),
                message: "priority cannot be null".to_string(),
            });
            None
        }
        Some(serde_json::Value::String(s)) => {
            match Priority::from_str(&s) {
                Some(prio) => Some(prio),
                None => {
                    details.push(ValidationError {
                        field: "priority".to_string(),
                        message: "priority must be exactly 'Low', 'Medium', or 'High'".to_string(),
                    });
                    None
                }
            }
        }
        Some(_) => {
            details.push(ValidationError {
                field: "priority".to_string(),
                message: "priority must be a string".to_string(),
            });
            None
        }
    };

    // 5. Parse tags (optional, handles null and non-string errors)
    let mut tags = Vec::new();
    if let Some(tags_val) = payload.tags {
        match tags_val {
            serde_json::Value::Null => {} // null -> empty array gracefully
            serde_json::Value::Array(arr) => {
                for (idx, item) in arr.iter().enumerate() {
                    match item {
                        serde_json::Value::String(t) => {
                            let trimmed = t.trim().to_string();
                            if !trimmed.is_empty() {
                                tags.push(trimmed);
                            }
                        }
                        _ => {
                            details.push(ValidationError {
                                field: format!("tags[{}]", idx),
                                message: "tag must be a string".to_string(),
                            });
                        }
                    }
                }
            }
            _ => {
                details.push(ValidationError {
                    field: "tags".to_string(),
                    message: "tags must be an array of strings".to_string(),
                });
            }
        }
    }

    // 6. Validate file_id reference
    let mut file_info = None;
    if let Some(file_id_val) = payload.file_id {
        match file_id_val {
            serde_json::Value::Null => {}
            serde_json::Value::String(s) => {
                if !s.is_empty() {
                    match Uuid::parse_str(&s) {
                        Ok(uid) => {
                            // Resolve metadata from Postgres or Memory
                            let meta = {
                                let mut resolved = None;
                                if let Some(ref pool) = state.db {
                                    if let Ok(Some(row)) = sqlx::query(
                                        "SELECT file_id, file_name, mime_type, size_bytes FROM file_uploads WHERE file_id = $1"
                                    )
                                    .bind(uid)
                                    .fetch_optional(pool)
                                    .await
                                    {
                                        resolved = Some(UploadedFile {
                                            file_id: row.get("file_id"),
                                            file_name: row.get("file_name"),
                                            mime_type: row.get("mime_type"),
                                            size_bytes: row.get::<i64, _>("size_bytes") as usize,
                                        });
                                    }
                                }
                                if resolved.is_none() {
                                    resolved = state.uploads_mem.read().await.get(&uid).map(|m| UploadedFile {
                                        file_id: m.file_id,
                                        file_name: m.file_name.clone(),
                                        mime_type: m.mime_type.clone(),
                                        size_bytes: m.size_bytes,
                                    });
                                }
                                resolved
                            };

                            if let Some(info) = meta {
                                file_info = Some(info);
                            } else {
                                details.push(ValidationError {
                                    field: "file_id".to_string(),
                                    message: format!("file_id {} does not reference an uploaded file", uid),
                                });
                            }
                        }
                        Err(_) => {
                            details.push(ValidationError {
                                field: "file_id".to_string(),
                                message: "file_id must be a valid UUID".to_string(),
                            });
                        }
                    }
                }
            }
            _ => {
                details.push(ValidationError {
                    field: "file_id".to_string(),
                    message: "file_id must be a string".to_string(),
                });
            }
        }
    }

    if !details.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Validation failed".to_string(),
                details,
            }),
        )
            .into_response());
    }

    let submission = Submission {
        id: Uuid::new_v4(),
        title: title.unwrap(),
        description: description.unwrap(),
        category: category.unwrap(),
        priority: priority.unwrap(),
        tags: tags.clone(),
        file_info: file_info.clone(),
        created_at: Utc::now(),
    };

    // Save to Database (Postgres) or Memory fallback
    if let Some(ref pool) = state.db {
        let tag_array = tags;
        if let Err(e) = sqlx::query(
            r#"
            INSERT INTO submissions (id, title, description, category, priority, tags, file_id, file_name, file_mime_type, file_size_bytes, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#
        )
        .bind(submission.id)
        .bind(&submission.title)
        .bind(&submission.description)
        .bind(serde_json::to_string(&submission.category).unwrap_or_default().replace('"', ""))
        .bind(format!("{:?}", submission.priority))
        .bind(&tag_array)
        .bind(file_info.as_ref().map(|f| f.file_id))
        .bind(file_info.as_ref().map(|f| &f.file_name))
        .bind(file_info.as_ref().map(|f| &f.mime_type))
        .bind(file_info.as_ref().map(|f| f.size_bytes as i64))
        .bind(submission.created_at)
        .execute(pool)
        .await
        {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to persist submission in Postgres".to_string(),
                    details: vec![ValidationError {
                        field: "database".to_string(),
                        message: e.to_string(),
                    }],
                }),
            )
                .into_response());
        }
    } else {
        state.submissions_mem.write().await.push(submission.clone());
    }

    Ok((StatusCode::CREATED, Json(submission)))
}

// GET /api/submissions
async fn list_submissions(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(10);

    // Retrieve submissions from Postgres first, falling back to memory
    let db_submissions = if let Some(ref pool) = state.db {
        match sqlx::query(
            "SELECT id, title, description, category, priority, tags, file_id, file_name, file_mime_type, file_size_bytes, created_at FROM submissions"
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                let mut list = Vec::new();
                for row in rows {
                    let id: Uuid = row.get("id");
                    let title: String = row.get("title");
                    let description: String = row.get("description");
                    let category_str: String = row.get("category");
                    let priority_str: String = row.get("priority");
                    let tags: Vec<String> = row.get("tags");
                    let file_id: Option<Uuid> = row.get("file_id");
                    let file_name: Option<String> = row.get("file_name");
                    let file_mime_type: Option<String> = row.get("file_mime_type");
                    let file_size_bytes: Option<i64> = row.get("file_size_bytes");
                    let created_at: DateTime<Utc> = row.get("created_at");

                    let category = Category::from_str(&category_str).unwrap_or(Category::Other);
                    let priority = Priority::from_str(&priority_str).unwrap_or(Priority::Medium);

                    let file_info = if let (Some(fid), Some(fname), Some(fmime), Some(fsize)) = (file_id, file_name, file_mime_type, file_size_bytes) {
                        Some(UploadedFile {
                            file_id: fid,
                            file_name: fname,
                            mime_type: fmime,
                            size_bytes: fsize as usize,
                        })
                    } else {
                        None
                    };

                    list.push(Submission {
                        id,
                        title,
                        description,
                        category,
                        priority,
                        tags,
                        file_info,
                        created_at,
                    });
                }
                list
            }
            Err(e) => {
                tracing::error!("Failed to fetch submissions from Postgres: {}", e);
                Vec::new()
            }
        }
    } else {
        state.submissions_mem.read().await.clone()
    };

    // Apply filtering
    let mut filtered = db_submissions;

    if let Some(cat_filter) = params.category {
        if !cat_filter.trim().is_empty() {
            filtered.retain(|s| {
                serde_json::to_string(&s.category)
                    .unwrap_or_default()
                    .replace('"', "")
                    .eq_ignore_ascii_case(&cat_filter)
            });
        }
    }

    if let Some(prio_filter) = params.priority {
        if !prio_filter.trim().is_empty() {
            filtered.retain(|s| {
                format!("{:?}", s.priority).eq_ignore_ascii_case(&prio_filter)
            });
        }
    }

    if let Some(search_filter) = params.search {
        let search_filter = search_filter.trim().to_lowercase();
        if !search_filter.is_empty() {
            filtered.retain(|s| {
                s.title.to_lowercase().contains(&search_filter)
                    || s.description.to_lowercase().contains(&search_filter)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&search_filter))
            });
        }
    }

    // Sort by created_at descending (latest first)
    filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let total = filtered.len();
    let start = std::cmp::min(offset, total);
    let end = std::cmp::min(start + limit, total);
    let items = filtered[start..end].to_vec();

    Json(SubmissionListResponse {
        items,
        total,
        offset,
        limit,
    })
}
