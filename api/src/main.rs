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
    pub submissions: Arc<RwLock<Vec<Submission>>>,
    pub uploads: Arc<RwLock<HashMap<Uuid, UploadMetadata>>>,
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
    // Set up logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .init();

    // Create uploads directory
    tokio::fs::create_dir_all("./uploads")
        .await
        .expect("Failed to create uploads directory");

    // Initialize state
    let state = AppState {
        submissions: Arc::new(RwLock::new(Vec::new())),
        uploads: Arc::new(RwLock::new(HashMap::new())),
    };

    // Configure CORS
    let allowed_origins_env = std::env::var("ALLOWED_ORIGINS").ok();
    let cors = if let Some(origins_str) = allowed_origins_env {
        let mut origins = Vec::new();
        for origin in origins_str.split(',') {
            if let Ok(value) = origin.trim().parse::<axum::http::HeaderValue>() {
                origins.push(value);
            }
        }
        if origins.is_empty() {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    } else {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    // Build routes
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/upload", post(upload_file))
        .route("/api/files/{id}", get(get_file))
        .route("/api/submissions", post(create_submission).get(list_submissions))
        .layer(cors)
        .with_state(state);

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
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
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

    tokio::fs::write(&storage_path, &file_data).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to save file".to_string(),
                details: vec![ValidationError {
                    field: "file".to_string(),
                    message: e.to_string(),
                }],
            }),
        )
            .into_response()
    })?;

    let meta = UploadMetadata {
        file_id,
        file_name: file_name.clone(),
        mime_type: content_type.clone(),
        size_bytes: file_data.len(),
        storage_path,
    };

    state.uploads.write().await.insert(file_id, meta);

    Ok((
        StatusCode::OK,
        Json(UploadedFile {
            file_id,
            file_name,
            mime_type: content_type,
            size_bytes: file_data.len(),
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

    let uploads = state.uploads.read().await;
    if let Some(meta) = uploads.get(&file_id) {
        match tokio::fs::read(&meta.storage_path).await {
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
                            field: "file".to_string(),
                            message: e.to_string(),
                        }],
                    }),
                )
                    .into_response()
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
                            let uploads = state.uploads.read().await;
                            if let Some(meta) = uploads.get(&uid) {
                                file_info = Some(UploadedFile {
                                    file_id: meta.file_id,
                                    file_name: meta.file_name.clone(),
                                    mime_type: meta.mime_type.clone(),
                                    size_bytes: meta.size_bytes,
                                });
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
        tags,
        file_info,
        created_at: Utc::now(),
    };

    state.submissions.write().await.push(submission.clone());

    Ok((StatusCode::CREATED, Json(submission)))
}

// GET /api/submissions
async fn list_submissions(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(10);

    let list = state.submissions.read().await;

    // Apply filtering
    let mut filtered: Vec<Submission> = list.clone();

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
