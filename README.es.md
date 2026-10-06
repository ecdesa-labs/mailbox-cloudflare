# mailbox-cloudflare

🌐 [Português (Brasil)](README.pt-BR.md) · [English](README.en.md)

Un buzón de correo personal que funciona por completo en Cloudflare: recibe,
guarda, indexa, envía y responde correos de uno o más de tus dominios, sin
servidor de correo propio y sin proveedor de buzón de pago.

> **Estado:** en desarrollo. Recibe, procesa e indexa correos reales (R2 + D1,
> con hilos y búsqueda), y tiene una API y una aplicación web para leer,
> organizar, escribir y responder (ver [Hoja de ruta](#hoja-de-ruta)).

> Proyecto independiente, sin vínculo con Cloudflare, Inc. "Cloudflare" es una
> marca de Cloudflare, Inc. y aparece en el nombre solo para indicar la
> plataforma.

## Propuesta

- **Uso propio.** Un único dueño, varios dominios, una bandeja unificada con
  vista por dominio.
- **Solo Cloudflare.** Email Routing entrega cada mensaje a un Worker; R2
  guarda los archivos; D1 guarda el índice; Access protege el acceso; Email
  Sending envía. Recibir cabe en el plan gratuito para volumen personal; enviar
  requiere el plan Workers Paid.
- **El `.eml` original es la fuente de la verdad.** Todo lo que está en la base
  de datos se deriva de él y puede reprocesarse en cualquier momento.
- **Reutilizable.** Dominios, colores y nombres de los recursos están en un
  único archivo de instancia. Cualquiera puede desplegar su propio buzón con
  un comando.

## Cómo funciona

```
Correo → Email Routing (ruta por dirección o catch-all)
           → Worker [email]  guarda el .eml en R2 y lo encola
           → Queue mail-ingest (+ DLQ)
           → Worker [queue]  analiza, guarda adjuntos y HTML saneado en R2, indexa en D1

Navegador → Cloudflare Access → Worker [fetch]
              ├─ /api/*  → D1 / R2
              └─ /*      → aplicación web estática (Workers Assets)
```

Un único Worker, escrito en Rust ([workers-rs](https://github.com/cloudflare/workers-rs)),
con tres handlers:

| Handler | Función |
|---|---|
| `email` | Recibe de Email Routing. Solo guarda el `.eml` y lo encola: rápido y sin análisis. |
| `queue` | Hace el trabajo pesado: MIME, adjuntos, saneamiento de HTML, hilos, reglas. Los fallos repetidos van a la DLQ; el `.eml` sigue en R2 para reprocesar. |
| `fetch` | API JSON en `/api/*` y la aplicación web estática en el resto, en el mismo origen (sin CORS). |

**Almacenamiento**

- **R2**: `{dominio}/raw/{aaaa}/{mm}/{id}.eml`, adjuntos en `{dominio}/att/…`
  y HTML saneado en `{dominio}/html/…`. El prefijo por dominio facilita la
  retención y la eliminación.
- **D1** (SQLite): dominios, identidades, hilos, mensajes, adjuntos, reglas de
  enrutamiento y búsqueda de texto completo (FTS5, mantenida por triggers).

**Seguridad**

- El HTML de los correos se trata como entrada hostil: se sanea durante el
  procesamiento, se muestra en un `<iframe sandbox>` sin scripts y con las
  imágenes remotas bloqueadas por defecto (evita el rastreo).
- Todo acceso pasa por Cloudflare Access; el Worker valida el JWT.

## Cómo usarlo

**Para instalarlo, sigue la [guía de instalación](docs/instalacion.es.md).**
Va desde cero (cuenta y dominio en Cloudflare) hasta un buzón que recibe y
envía, en orden, con lo que hay que hacer en el panel de Cloudflare y en la
terminal, y el porqué de cada paso.

El resumen de abajo es una referencia para quien ya lo instaló.

### Varios dominios

`--label` y `--color` siguen el orden de los `--domain`:

```bash
just configure \
  --domain ejemplo.com --label Ejemplo --color '#2563EB' \
  --domain otro.dev    --label Otro    --color '#E5484D'
```

Una vez aplicado el seed en una base D1, un dominio nuevo entra mediante una
migración nueva.

### Nombres de los recursos

Por defecto: Worker y R2 `mailbox`, D1 `mailbox`, colas `mail-ingest` y
`mail-ingest-dlq`. Para tener más de una instancia en la misma cuenta, cambia
los nombres:

```bash
just configure --worker buzon --bucket buzon --database buzon --queue buzon-in --dlq buzon-dlq
```

Todas las opciones: `just configure --help`.

### Comandos

| Comando | Qué hace |
|---|---|
| `just up [args]` | Todo, de punta a punta |
| `just doctor` | Revisa las herramientas del equipo |
| `just configure [args]` | Configura la instancia |
| `just login` | Inicia sesión en Cloudflare |
| `just bootstrap` | Crea los recursos remotos que falten |
| `just ci` | fmt, clippy, pruebas, regla de dependencias, build wasm, verificación de la instancia |
| `just migrate-local` / `just migrate-remote` | Migraciones de D1 |
| `just deploy` | CI + migración remota + publicación + prueba de humo |
| `just dev` | Ejecuta en local con `wrangler dev` (D1, R2 y colas simulados) |
| `just contract` | Pruebas de contrato de la API con Worker, D1, R2 y colas locales |
| `just front-dev` | Aplicación web en local con datos de ejemplo (sin API), en `localhost:3000` |
| `just front-build` | Genera la aplicación web estática en `mailbox/web/out` (`deploy` ya lo hace) |
| `just email-enable` | Activa Email Routing en los dominios de la instancia (reemplaza los MX) |
| `just email-route <dirección>` | Envía una dirección específica al Worker |
| `just email-address <dirección> ["nombre"]` | Ruta + identidad: habilita una dirección nueva para recibir y enviar |
| `just email-catchall` | Envía todas las direcciones de los dominios al Worker |
| `just email-status` | Estado de Email Routing y de las reglas |
| `just reprocess --key <raw_key> \| --failed \| --all` | Pide reprocesamiento (el cron lo ejecuta en hasta 10 min) |
| `just failures` | Fallos de procesamiento y pedidos pendientes |

## La instancia

Todo lo que cambia de una persona a otra está en `mailbox/instance.json`, que
**no va a git**: contiene el id de tu D1, el equipo y la aplicación de Access y
tus dominios. La plantilla versionada es
[`mailbox/instance.example.json`](mailbox/instance.example.json):

```json
{
  "worker": "mailbox",
  "bucket": "mailbox",
  "database": "mailbox",
  "database_id": "00000000-0000-0000-0000-000000000000",
  "queue": "mail-ingest",
  "dlq": "mail-ingest-dlq",
  "access_team": "",
  "access_aud": "",
  "allowed_emails": [],
  "metrics": false,
  "plan": "free",
  "domains": [
    { "id": "01M…", "name": "ejemplo.com", "label": "Ejemplo", "color": "#2563EB" }
  ]
}
```

No hace falta copiarlo a mano: el primer `just configure --domain ...` crea el
tuyo.

[`scripts/setup_instance.py`](scripts/setup_instance.py) actualiza ese archivo
y genera, a partir de [`mailbox/templates/`](mailbox/templates):

- `mailbox/wrangler.toml`
- `mailbox/migrations/0002_seed_domains.sql`
- `mailbox/instance.env` (leído por los `justfile`)

Esos tres tampoco van a git. No los edites a mano: `just ci` falla si se
apartan de `instance.json`. Para cambiar algo en `wrangler.toml`, edita la
plantilla. Ninguno guarda secretos (el token de la API está en la sesión de
wrangler, fuera del proyecto), pero identifican tu cuenta.

## Operación

- **Retención:** la papelera y el spam se borran definitivamente (con el
  `.eml`) 30 días después de llegar ahí; los mensajes ocultos por una regla
  `reject`, 30 días después de ocultarse. Se ajusta en `[vars]` de la plantilla
  (`MAILBOX_RETENTION_*_DAYS`; `0` = nunca). Corre en el mismo cron que la
  reconciliación.
- **Logs:** cada evento sale en JSON con `level` y `event` (`ingest`,
  `process`, `send`, `dlq`, `reconcile`, `retention`, `auth`, `api_error`).
  Se ven en **Workers & Pages → mailbox → Logs** o con `npx wrangler tail`.
- **Métricas (opcional):** Analytics Engine, dataset `<worker>_events`. Vienen
  desactivadas: la cuenta debe activar Analytics Engine en el panel antes (si
  no, el despliegue falla con el código 10089). Después, `just configure
  --metrics on` y `just deploy`. Cada punto tiene `index1` = evento,
  `blob1..3` = evento, resultado y dominio, `double1` = tamaño en bytes y
  `double2` = cantidad. Ejemplo (SQL API de Analytics Engine):

  ```sql
  SELECT blob1 AS evento, blob2 AS resultado, SUM(_sample_interval * double2) AS total
  FROM mailbox_events WHERE timestamp > NOW() - INTERVAL '1' DAY
  GROUP BY evento, resultado ORDER BY total DESC
  ```

- **API:** la especificación OpenAPI 3.1 está en `/api/openapi.json` (fuente:
  [`mailbox/worker/openapi.json`](mailbox/worker/openapi.json)). `just
  contract` comprueba que cubre todas las rutas y que cada respuesta coincide
  con el esquema.

## Estructura del repositorio

```
mailbox-cloudflare/
  justfile              flujo de punta a punta
  docs/                 guía de instalación (pt-BR, en y es)
  scripts/              setup_instance.py, email_address.py, contract_test.py y pruebas
  mailbox/              el producto
    crates/
      domain/           entidades, value objects y reglas; puro, probado en nativo
      application/      casos de uso y puertos (traits)
      mime/             .eml → mensaje analizado, saneamiento de HTML; puro
      adapters-cf/      D1, R2, Queue, Mailer; solo wasm32
    worker/             los tres handlers y el montaje de los adaptadores
    migrations/         SQL de D1
    templates/          base de los archivos generados desde la instancia
    web/                aplicación web (Next.js estático; prompts de v0 en web/prompts/)
    instance.example.json  plantilla de la instancia (tu instance.json no va a git)
    docs/               notas locales (no van a git)
  mailbox-spike/        prueba de concepto que validó la plataforma
```

Arquitectura hexagonal: `domain` ← `application` ← `adapters-cf` ← `worker`.
`domain`, `application` y `mime` no dependen de `worker` (`just ci` lo
verifica), así que se prueban en nativo, sin Cloudflare.

## Hoja de ruta

| Fase | Entrega | Estado |
|---|---|---|
| 0 | Base: workspace, wrangler, CI | ✅ |
| 1 | Dominio, migraciones, value objects; configuración de la instancia | ✅ |
| 2 | Crate `mime`: análisis, saneamiento, `cid:` e imágenes remotas | ✅ |
| 3 | Handler `email`: recibir de verdad | ✅ |
| 4 | Handler `queue`: procesar, idempotente, DLQ, modo ligero, reconciliación | ✅ |
| 5 | API de lectura con paginación por cursor | ✅ |
| 6 | Autenticación con Cloudflare Access | ✅ |
| 7 | Búsqueda, marcas, carpetas, acciones en lote, reglas de enrutamiento | ✅ |
| 8 | Responder y enviar (Cloudflare Email Sending) | ✅ |
| 9 | Retención, métricas, logs estructurados, OpenAPI | ✅ |

### Aplicación web

Next.js estático en `mailbox/web`, servido por el mismo Worker.

| Fase | Entrega | Estado |
|---|---|---|
| 1 | Estructura, sistema de diseño, `lib/api` con modo simulado | ✅ |
| 2 | Lista: búsqueda, selección en lote, acciones con deshacer | ✅ |
| 3 | Lector: HTML aislado, imágenes remotas bajo demanda, adjuntos | ✅ |
| 4 | Hilos: conversación agrupada, mensajes plegables | ✅ |
| 5 | Atajos de teclado y ayuda (`?`) | ✅ |
| 6 | Redactar y responder | ✅ |
| 7 | Pantalla de reglas de enrutamiento | |
| 8 | Ajustes para móvil | |

### Mejoras futuras

- [x] **Activar las métricas en Analytics Engine** (definido en la Fase 9). El
  producto debe activarse en la cuenta antes del primer despliegue con el
  binding (si no: código 10089). La activación puede tardar unos minutos;
  después, `just configure --metrics on` y `just deploy`.

- [ ] **Juegos de caracteres asiáticos multibyte** (Shift_JIS, ISO-2022-JP,
  GB2312, GBK, Big5, EUC-KR). Hoy se convierten en texto con caracteres de
  sustitución y el mensaje queda marcado con `has_encoding_problem`. Camino:
  activar la función `full_encoding` de `mail-parser` (incluye `encoding_rs`),
  medir el aumento del `.wasm` y añadir goldens con esos juegos de caracteres.
- [ ] **CSS en bloques `<style>`.** Hoy el `<style>` se elimina por completo
  (`ammonia` no sanea hojas de estilo), así que los correos de marketing
  pierden el diseño adaptable; los estilos en línea (`style=""`) se mantienen.
  Camino: sanear la hoja con `cssparser` (la misma lista de propiedades sin
  `url()`, sin `@import`, `@font-face` ni selectores que escapen del correo) o
  convertir las reglas en estilos en línea durante el procesamiento y pasarlas
  por el filtro actual. En ambos casos, el CSS no puede cargar nada externo.

## Limitaciones conocidas

- Email Routing acepta mensajes de hasta 25 MiB.
- Email Sending (beta pública): hasta 5 MiB por mensaje y 50 destinatarios
  (Para + Cc); la cuota diaria empieza conservadora y crece con el uso. El
  `Message-ID` que recibe el destinatario lo genera Cloudflare; la copia en
  Enviados usa el id que devuelve el servicio. El envío todavía no admite
  adjuntos.
- Recibir y guardar no depende del tamaño (el `.eml` va por stream a R2).
  Procesar sí: los correos por encima de un umbral pasan al **modo ligero**
  (remitente, asunto y fecha; el `.eml` queda para descargar), y los correos
  con demasiados adjuntos tienen el texto indexado, pero los adjuntos y el HTML
  solo en el `.eml`. Los umbrales siguen el plan de la instancia (`just
  configure --plan free|paid`):

  | | Free | Paid |
  |---|---|---|
  | Modo ligero por encima de | 512 KiB (10 ms de CPU) | 16 MiB (128 MB de memoria) |
  | Máximo de adjuntos procesados | 30 | 200 |
  | Reconciliación por ejecución | 50 | 500 |
  | Retención por ejecución | 50 | 500 |

  En Paid, el pico de memoria del wasm medido fue ~2,5 veces el tamaño del
  `.eml`; el evento `process` incluye `wasm_mib` para recalibrar.
- En el plan Free, los mensajes de la cola y de la DLQ caducan en 24 h; el
  reprocesamiento siempre parte del `.eml` en R2.
- `wrangler d1 export` no exporta tablas virtuales (la búsqueda FTS5). Para
  exportar la base, borra `messages_fts`, exporta y vuelve a crearla. La copia
  de seguridad real es el bucket de R2 con los `.eml`.

La interfaz del buzón está, por ahora, en portugués (Brasil).
