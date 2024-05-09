## Db服务

我们需要将用户的消息数据保存到数据库中，注意，这里我们只做一个demo级别的学习项目，实际项目中会有大量的业务相关的代码，这里我们只将消息保存到数据库中，不做其他操作，更加详细的代码请自行查阅。

### 创建项目并引入相关依赖

```shell
cargo new db

cargo add abi -p db 
cargo add utils -p db
cargo add sqlx -p db --features=runtime-tokio-rustls --features=postgres --features=chrono 
cargo add async-trait -p db
cargo add tonic -p db --features=gzip
cargo add tokio -p db --features=full
cargo add tracing -p db  
cargo add tracing_subscriber -p db
```

### 创建文件

```shell
mkdir db/src/database
mkdir db/src/database/postgres

touch db/src/database/mod.rs
touch db/src/database/postgres/mod.rs
```

**导出模块：**

db/src/main.rs

```rust
mod database;
```

db/src/database/mod.rs

```rust
mod postgres;
```

### 安装sqlx-cli

我们需要创建数据库表，所以这里使用sqlx-cli来协助我们

```shell
cargo install sqlx-cli

# 添加sqlx配置文件
touch .env
```

**配置sqlx-cli:**

.env

```
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/im
```

### 创建messages表

```shell
mkdir migrations

# 添加messages，如果你希望能够回滚那么增加-r参数
sqlx migrate add messages
```

这时候会在`migrations`文件夹下自动生成一个后缀为`_messages.sql`的文件，在文件中加入我们的表结构：

migrations/xxx_messages.sql

```sql
CREATE TABLE messages
(
    send_id      VARCHAR NOT NULL,
    receiver_id  VARCHAR NOT NULL,
    local_id     VARCHAR NOT NULL,
    server_id    VARCHAR NOT NULL,
    send_time    BIGINT  NOT NULL,
    msg_type     INT,
    content_type INT,
    content      TEXT,
    PRIMARY KEY (send_id, server_id, send_time)
);
```

**执行migrate**

```shell
sqlx migrate run
```



### 定义数据库trait

db/src/database/mod.rs

```rust
mod postgres;

use std::sync::Arc;

use abi::config::Config;
use async_trait::async_trait;

use abi::errors::Error;
use abi::message::Msg;

use self::postgres::PostgresMessage;

/// face to postgres db
#[async_trait]
pub trait MsgStoreRepo: Sync + Send {
    /// save message to db
    async fn save_message(&self, message: Msg) -> Result<(), Error>;
}

pub async fn msg_repo(config: &Config) -> Arc<dyn MsgStoreRepo> {
    Arc::new(PostgresMessage::from_config(config).await)
}
```

### 实现postgres

db/src/database/postgres/mod.rs

```rust
use abi::config::Config;
use async_trait::async_trait;
use sqlx::PgPool;

use abi::errors::Error;
use abi::message::Msg;

use crate::database::MsgStoreRepo;

pub struct PostgresMessage {
    pool: PgPool,
}

impl PostgresMessage {
    pub async fn from_config(config: &Config) -> Self {
        let pool = PgPool::connect(&config.db.url()).await.unwrap();
        Self { pool }
    }
}

#[async_trait]
impl MsgStoreRepo for PostgresMessage {
    async fn save_message(&self, message: Msg) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO messages
             (local_id, server_id, send_id, receiver_id, msg_type, content_type, content, send_time)
             VALUES
             ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT DO NOTHING",
        )
        .bind(&message.local_id)
        .bind(&message.server_id)
        .bind(&message.send_id)
        .bind(&message.receiver_id)
        .bind(message.msg_type)
        .bind(message.content_type)
        .bind(&message.content)
        .bind(message.send_time)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
```

这里使用了sqlx库中的query方法并使用bind方法将消息对象中的字段绑定到SQL查询的参数占位符上，在SQL中我们在最后使用了`ON CONFLICT DO NOTHING`来确保当server_id已经存在的话就不进行插入操作。最后因为我们在abi模块中已经做了sqlx::Error到自定义Error的类型转换，所以可以直接使用`?`来实现错误类型的转换。

**说明：**使用`sqlx`而不选择ORM的主要原因可能包括以下几点：

1. **直接和灵活**：`sqlx`提供了接近底层的SQL操作能力，允许更精细和直接的数据库交互控制，可以精确地编写和优化SQL语句。
2. **类型安全**：`sqlx`在编译时即检查SQL语句的正确性，减少运行时错误，确保了高度的类型安全。
3. **性能考虑**：不使用ORM可以避免额外的性能开销，`sqlx`执行直接的SQL查询通常比通过ORM层的抽象更快。
4. **简洁性**：`sqlx`提供了简洁的API，更加适合我们。
5. **避免过度抽象**：ORM往往带来过度的抽象层，这在复杂查询或特定的优化需求上可能导致不便。直接使用`sqlx`可以让开发者充分控制查询逻辑。

### 添加protobuf相关数据结构以及接口

abi/protos/messages.proto

```protobuf
message SaveMessageRequest {
  Msg message = 1;
}

message SaveMessageResponse {}

service DbService {
  rpc SaveMessage(SaveMessageRequest) returns (SaveMessageResponse);
}
```

### 定义DbRpcService

db/src/main

```rust
use std::sync::Arc;

use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{debug, info, Level};

use abi::config::Config;
use abi::errors::Error;
use abi::message::db_service_server::{DbService, DbServiceServer};
use abi::message::{SaveMessageRequest, SaveMessageResponse};
use utils::typos::Registration;

use crate::database::MsgStoreRepo;

/// DbRpcService contains the postgres trait
pub struct DbRpcService {
    db: Arc<dyn MsgStoreRepo>,
}

impl DbRpcService {
    pub async fn new(config: &Config) -> Self {
        Self {
            db: database::msg_repo(config).await,
        }
    }

    pub async fn start(config: &Config) {
        // register service
        Self::register_service(config).await.unwrap();
        info!("<db> rpc service health check started");

        let db_rpc = DbRpcService::new(config).await;
        let service = DbServiceServer::new(db_rpc);
        info!(
            "<db> rpc service started at {}",
            config.rpc.db.rpc_server_url()
        );

        Server::builder()
            .add_service(service)
            .serve(config.rpc.db.rpc_server_url().parse().unwrap())
            .await
            .unwrap();
    }

    /// 服务注册
    async fn register_service(config: &Config) -> Result<(), Error> {
        // register service to service register center
        let center = utils::service_register_center(config);

        let registration = Registration {
            id: format!("{}-{}", utils::get_host_name()?, &config.rpc.db.name),
            name: config.rpc.db.name.clone(),
            address: config.rpc.db.host.clone(),
            port: config.rpc.db.port,
            tags: config.rpc.db.tags.clone(),
        };
        center.register(registration).await?;
        Ok(())
    }
}
```

### 实现gRPC接口

```rust
#[async_trait::async_trait]
impl DbService for DbRpcService {
    async fn save_message(
        &self,
        request: Request<SaveMessageRequest>,
    ) -> Result<Response<SaveMessageResponse>, Status> {
        let inner = request.into_inner();
        let message = inner
            .message
            .ok_or_else(|| Status::invalid_argument("message is empty"))?;
        debug!("save message: {:?}", message);
        // 调用自己的save_message方法将消息落盘
        self.db
            .save_message(message)
            .await
            .map_err(|err| Status::internal(format!("Save Message Failed: {:?}", err)))?;
        return Ok(Response::new(SaveMessageResponse {}));
    }
}
```

### 修改main方法，启动db服务

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = abi::config::Config::load("./abi/config.yaml");
    DbRpcService::start(&config).await;
}
```

**总结：**我们构建了微服务架构中的数据库服务部分，并提供了gRPC接口，这样其他服务就可以很容易地存储信息到数据库。为了代码未来能够灵活地切换不同的数据库，也使用了trait来设计数据库层，增加了系统的可适配性。这样，不仅现在的服务运行良好，将来的改动也会更加顺利。