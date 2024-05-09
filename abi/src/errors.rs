type Message = String;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // 内部错误
    #[error("internal server errors")]
    InternalServer(Message),

    #[error("not found")]
    NotFound,

    #[error("redis error: {0}")]
    RedisError(redis::RedisError),

    #[error("database errors{0}")]
    DbError(sqlx::Error),

    #[error("reqwest error: {0}")]
    ReqwestError(reqwest::Error),
}

/// 将redis的error转为我们自己的Error类型
impl From<redis::RedisError> for Error {
    fn from(value: redis::RedisError) -> Self {
        Self::RedisError(value)
    }
}

/// 转换reqwest的error
impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(value)
    }
}

/// 转换sqlx中的error
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(e) => Error::DbError(sqlx::Error::Database(e)),
            sqlx::Error::RowNotFound => Error::NotFound,
            _ => Error::DbError(e),
        }
    }
}
