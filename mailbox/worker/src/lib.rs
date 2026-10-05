//! Casca: três handlers + montagem dos adaptadores. Stubs da Fase 0.
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get("/api/ping", |_, _| Response::ok("pong"))
        .run(req, env)
        .await
}

// Fase 3 implementa. Até lá rejeita, para nunca engolir email em silêncio.
#[event(email)]
async fn email(message: ForwardableEmailMessage, _env: Env, _ctx: Context) -> Result<()> {
    message.set_reject("mailbox not ready");
    Ok(())
}

// Fase 4 implementa. Até lá devolve para a fila (acaba na DLQ), sem descartar.
#[event(queue)]
async fn queue(batch: MessageBatch<serde_json::Value>, _env: Env, _ctx: Context) -> Result<()> {
    batch.retry_all();
    Ok(())
}
