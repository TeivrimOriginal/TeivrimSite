use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;
use std::net::SocketAddr;

async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("hello world")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    
    println!("Сервер запущен на http://{}", addr);
    
    // Создаем make_service, который будет создавать наш сервис для каждого соединения
    let make_svc = make_service_fn(|_conn| {
        async { Ok::<_, Infallible>(service_fn(hello_world)) }
    });
    
    let server = Server::bind(&addr).serve(make_svc);
    
    println!("Сервер запущен. Нажмите Ctrl+C для остановки.");
    
    server.await?;
    
    Ok(())
}