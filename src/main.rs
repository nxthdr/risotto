mod api;
mod bmp;
mod db;
mod router;
mod update;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let db = db::new_db().await;

    let db_api = db.clone();
    let api_handler = tokio::spawn(async move {
        let api_listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
        let app = api::app(db_api.clone());

        axum::serve(api_listener, app).await.unwrap();
    });

    let db_api = db.clone();
    let bmp_handler = tokio::spawn(async move {
        let bmp_listener = TcpListener::bind("0.0.0.0:4000").await.unwrap();
        loop {
            let (mut bmp_socket, _) = bmp_listener.accept().await.unwrap();
            let db_api = db_api.clone();
            tokio::spawn(async move {
                loop {
                    bmp::handle(&mut bmp_socket, db_api.clone()).await
                }
            });
        }
    });

    api_handler.await.unwrap();
    bmp_handler.await.unwrap();
}
