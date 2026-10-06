# Guía de instalación

🌐 [Português (Brasil)](instalacao.pt-BR.md) · [English](installation.en.md) · [Volver al README](../README.es.md)

Esta guía te lleva desde cero hasta un buzón de correo funcionando en tu
propio dominio, en tu propia cuenta de Cloudflare. No supone que conozcas el
proyecto: cada paso dice **qué hacer**, **dónde** (en el panel de Cloudflare o
en la terminal) y **por qué**.

Los pasos están en orden. Algunos dependen de los anteriores, así que sigue la
secuencia.

## Antes de empezar

**Qué tendrás al final**

- Una dirección como `https://mailbox.<tu-subdominio>.workers.dev`, protegida
  con inicio de sesión, donde lees, organizas y (opcionalmente) envías correos.
- Direcciones `algo@tudominio.com` que entregan en ese buzón.

**Cuánto cuesta**

| Para | Plan | Costo |
|---|---|---|
| Recibir y leer correos | Workers Free | Gratis para volumen personal |
| También enviar y responder | Workers Paid | US$ 5/mes (incluye 3.000 envíos/mes) |

R2 (donde se guardan los correos) y Zero Trust (el inicio de sesión) piden una
tarjeta de crédito al activarlos, incluso en los planes gratuitos. No se cobra
nada mientras estés dentro de las cuotas gratuitas.

**Cuánto tarda:** alrededor de una hora la primera vez, sobre todo esperando
instalaciones y la primera compilación.

**Atención si el dominio ya tiene correo:** activar la recepción reemplaza los
registros MX del dominio. Si hoy recibes correo de ese dominio en otro lugar
(Google Workspace, Zoho, el hosting de tu web…), ese lugar deja de recibirlo.
Migra esos buzones antes o usa un dominio que todavía no tenga correo.

---

## Parte 1: en el panel de Cloudflare

Estos pasos los haces tú en el navegador, en
[dash.cloudflare.com](https://dash.cloudflare.com). Activan los servicios que
usa el proyecto; sin ellos, los comandos de las partes siguientes fallan.

### 1.1 Crea la cuenta y activa la verificación en dos pasos

Crea una cuenta en [dash.cloudflare.com](https://dash.cloudflare.com/sign-up)
(o entra en la tuya). En **Perfil → Autenticación**, activa la verificación en
dos pasos.

> **Por qué:** esta cuenta va a guardar todos tus correos, y su inicio de
> sesión también protege el buzón (Parte 4).

### 1.2 Pon el dominio en Cloudflare

En el panel, haz clic en **Añadir → Dominio existente** (o "Add a domain"),
escribe el dominio y elige el plan **Free**. Cloudflare muestra dos
*nameservers* (algo como `ana.ns.cloudflare.com`). Entra en el sitio donde
compraste el dominio (tu registrador) y reemplaza los nameservers por los de
Cloudflare. La activación tarda de minutos a algunas horas; el panel avisa
cuando el dominio queda **Activo**.

> **Por qué:** la recepción de correo (Email Routing) solo funciona con el DNS
> del dominio en Cloudflare.

Si el dominio ya estaba en Cloudflare, salta este paso.

### 1.3 Activa R2

En el menú, abre **Almacenamiento y bases de datos → R2 → Información
general** (*Storage & databases → R2 → Overview*) y completa la activación (el
*checkout* del plan de R2, gratuito hasta 10 GB).

> **Por qué:** R2 guarda cada correo original (`.eml`), los adjuntos y el
> HTML. Es el archivo de tu buzón. Sin activarlo, la creación del bucket falla.

### 1.4 Elige tu subdominio `workers.dev`

Abre **Workers & Pages**. La primera vez, el panel pide elegir un subdominio;
si no, junto a **Your subdomain**, haz clic en **Change**. Elige un nombre
corto (por ejemplo, el de tu empresa).

> **Por qué:** es la dirección donde vivirá el buzón:
> `https://mailbox.<subdominio>.workers.dev`.

### 1.5 Activa Zero Trust (el inicio de sesión del buzón)

En el menú, abre **Zero Trust**. La primera vez pide:

1. Un **nombre de equipo** (*team name*): un identificador tuyo, como
   `miempresa`. Se convierte en la dirección de inicio de sesión
   `miempresa.cloudflareaccess.com`.
2. Un plan: elige **Free** (hasta 50 usuarios). Pide una tarjeta, pero no
   cobra.

> **Por qué:** Cloudflare Access (parte de Zero Trust) se pone delante del
> buzón y solo deja entrar a quien autorices. El proyecto rechaza todo acceso
> mientras no esté configurado.

### 1.6 (Solo si vas a enviar correos) Suscríbete a Workers Paid

En **Workers & Pages**, abre la opción de plan (**Plans** o **Usage model**) y
elige **Workers Paid**.

> **Por qué:** el envío de correos (Email Sending) a cualquier destinatario
> solo existe en el plan de pago. Para solo recibir, el plan gratuito basta;
> puedes suscribirte después.

---

## Parte 2: prepara el ordenador

A partir de aquí usarás la terminal (Linux o macOS). En Windows, usa
[WSL](https://learn.microsoft.com/windows/wsl/install) con Ubuntu y sigue las
instrucciones de Linux.

### 2.1 Instala las herramientas

| Herramienta | Para qué |
|---|---|
| git | descargar el proyecto |
| Rust (rustup) | compilar el Worker, que está escrito en Rust |
| Node.js (versión LTS) | ejecutar `wrangler`, la herramienta oficial de Cloudflare, y compilar la aplicación web |
| Python 3 | los scripts de configuración |
| just | los atajos de comandos (`just deploy`, `just up`…) |
| pkg-config y cabeceras de OpenSSL | dependencias de compilación |

**Ubuntu / Debian:**

```bash
sudo apt update
sudo apt install -y git python3 pkg-config libssl-dev build-essential curl just
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust; acepta lo predeterminado
```

Para Node.js, usa la versión LTS de [nodejs.org](https://nodejs.org) o de
[nvm](https://github.com/nvm-sh/nvm). Si tu Ubuntu no tiene el paquete `just`,
mira otras opciones en [just.systems](https://just.systems/man/en/packages.html).

**macOS** (con [Homebrew](https://brew.sh)):

```bash
brew install git python node just pkg-config openssl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Cierra y vuelve a abrir la terminal después de instalar Rust.

### 2.2 Descarga el proyecto y prepara la compilación

```bash
git clone <dirección del repositorio> mailbox-cloudflare
cd mailbox-cloudflare
just setup    # instala el destino WebAssembly de Rust y worker-build (una vez)
just doctor   # comprueba que no falte nada
```

`just doctor` muestra cada herramienta como `ok` o `FALTA` (falta) y dice cómo
resolver lo que falte. Sigue solo cuando todo esté en `ok`.

> **Por qué:** el Worker se ejecuta como WebAssembly dentro de Cloudflare;
> `setup` instala lo que convierte el código Rust a ese formato.

Ejecuta todos los comandos `just` siguientes en esta carpeta
(`mailbox-cloudflare`).

---

## Parte 3: configura y publica

### 3.1 Indica al proyecto cuál es tu dominio

```bash
just configure --domain tudominio.com --label "Mi Empresa" --color '#2563EB'
```

- `--label`: el nombre corto que aparece en la barra lateral del buzón.
- `--color`: el color del dominio en el buzón (hexadecimal).
- Si te suscribiste a Workers Paid (1.6), añade `--plan paid`: con más CPU y
  memoria disponibles, el buzón procesa por completo correos mucho más grandes.

> **Por qué:** todo lo que es tuyo (dominio, nombres, ids) está en un archivo
> local, `mailbox/instance.json`, que no va a git. Este comando crea ese
> archivo y genera a partir de él la configuración del Worker.

### 3.2 Conecta la terminal a tu cuenta

```bash
just login
```

Abre el navegador para que autorices a `wrangler` a gestionar tu cuenta. Al
final, la terminal muestra el correo y el id de la cuenta.

### 3.3 Crea los recursos en Cloudflare

```bash
just bootstrap
```

Crea el bucket de R2 (los archivos), la base de datos D1 (el índice: carpetas,
hilos, búsqueda) y las dos colas (procesamiento y fallos), y guarda el id de
la base en tu configuración. Ejecutarlo de nuevo no recrea nada.

### 3.4 Publica

```bash
just deploy
```

Ejecuta las pruebas, compila la aplicación web y el Worker, crea las tablas en
la base y publica. La primera vez tarda unos minutos. Al final aparece:

```
ok: https://mailbox.<subdominio>.workers.dev está no ar
```

Si abres esa dirección ahora, el buzón todavía no te deja entrar: rechaza todo
hasta que el inicio de sesión esté configurado (Parte 4). Es intencional.

> **Atajo:** `just up --domain ... --label ... --color ...` hace del 3.1 al
> 3.4 de una vez. Los pasos por separado ayudan a entender y a encontrar el
> problema si algo falla.

---

## Parte 4: protege el buzón con inicio de sesión

### 4.1 Activa Access en el Worker (panel)

1. Abre **Workers & Pages** y haz clic en el Worker **mailbox**.
2. Abre la pestaña **Domains**.
3. En la fila de `workers.dev`, haz clic en **Enable Cloudflare Access**. El
   panel crea una política que deja entrar a los miembros de tu cuenta de
   Cloudflare (tú). Para dejar entrar a otras personas o cambiar la forma de
   entrar, ajústala después en **Zero Trust → Access**.
4. La ventana que aparece muestra dos valores. **Copia los dos:**
   - el **team domain** (`https://miempresa.cloudflareaccess.com`);
   - el **AUD** (*Application Audience tag*, un texto largo en hexadecimal).

### 4.2 Pasa esos valores al proyecto y publica de nuevo

```bash
just configure --access-team miempresa.cloudflareaccess.com --access-aud <AUD copiado> --allowed-email <correo de tu cuenta de Cloudflare>
just deploy
```

**Qué correo va en `--allowed-email`:** el mismo con el que entras al panel de
Cloudflare (el correo de la cuenta). Es el que Access le informa al buzón
después del inicio de sesión. Para comprobarlo, ejecuta `just login`: la
terminal muestra *"logged in … associated with the email …"*. En el panel,
también aparece en el menú del perfil (esquina superior derecha).

- Es opcional: sin él, solo vale la política de Access. Con él, el buzón
  rechaza cualquier inicio de sesión que no sea de ese correo, aunque la
  política de Access lo deje pasar. Es un segundo candado.
- Si el correo no coincide con el de la cuenta, **te quedas fuera**: el inicio
  de sesión de Access funciona, pero justo después de entrar el buzón muestra
  "Sua sessão expirou" (tu sesión expiró) y no sale de ahí. Corrígelo con el
  `just configure` de arriba y `just deploy`.
- Si algún día dejas entrar a otra persona en la política de Access, añade
  también su correo repitiendo la opción: `--allowed-email tu@… --allowed-email ella@…`.

> **Por qué:** Access pide el inicio de sesión antes de llegar al buzón, y el
> Worker comprueba de nuevo, por su cuenta, que el inicio de sesión es válido
> y viene de tu Access. Si alguna de las dos partes no está bien, nada se abre.

### 4.3 Entra en el buzón

Abre `https://mailbox.<subdominio>.workers.dev`. Cloudflare te pide
identificarte (con tu cuenta de Cloudflare, según la política creada en 4.1).
Después aparece el buzón, todavía vacío.

---

## Parte 5: recibe correos

### 5.1 Activa la recepción en el dominio

```bash
just email-enable
```

Activa Email Routing y reemplaza los registros MX del dominio por los de
Cloudflare. (En el panel, el equivalente es **Compute → Email Service → Email
Routing → Onboard Domain**.)

Si el comando se rechaza por un **Null MX** (`MX 0 .`) o un SPF
`v=spf1 -all` (configuración de "dominio que no usa correo"), borra ese MX y
cambia ese SPF por `v=spf1 include:_spf.mx.cloudflare.net ~all` en **DNS**, en
el panel del dominio, y ejecútalo de nuevo.

### 5.2 Crea la primera dirección

```bash
just email-address contacto@tudominio.com "Mi Empresa"
```

Crea dos cosas: la **ruta**, que envía los correos de esa dirección a tu
buzón, y la **identidad**, que hace que la dirección aparezca en el campo
**De** (remitente) al escribir. El texto entre comillas es el nombre que ven
los destinatarios.

> **Por qué:** el dominio queda cerrado. Solo reciben correo las direcciones
> que creaste, lo que evita spam en direcciones inventadas (`admin@`,
> `ventas@`…). Repite el comando para cada dirección nueva.
>
> **Alternativa:** `just email-catchall` hace que *cualquier* dirección del
> dominio llegue al buzón, sin crearlas una por una. Es más práctico, pero
> cualquier dirección que inventen los spammers también entra.

### 5.3 Pruébalo

Desde otra cuenta de correo (Gmail, por ejemplo), envía un mensaje a la
dirección que creaste. En menos de un minuto aparece en la bandeja de entrada.

---

## Parte 6: envía correos (opcional, plan de pago)

Requiere Workers Paid (paso 1.6).

### 6.1 Verifica el dominio para el envío (panel)

1. Abre **Compute → Email Service → Email Sending**.
2. Haz clic en **Onboard Domain** y elige tu dominio.
3. Cloudflare crea por sí sola los registros de autenticación: DKIM
   (`cf-bounce._domainkey`) y SPF/MX del subdominio `cf-bounce`.
4. En **DNS**, comprueba que el registro `_dmarc` del dominio sigue como
   quieres. Si no existe, un buen comienzo es `v=DMARC1; p=quarantine;`.

> **Por qué:** sin estas firmas, Gmail y Outlook tratan tus correos como
> sospechosos o los rechazan. Con ellas, tus correos salen autenticados (SPF,
> DKIM y DMARC aprobados).

### 6.2 Si aún no lo hiciste, ajusta el plan en el proyecto

```bash
just configure --plan paid
just deploy
```

### 6.3 Pruébalo

En el buzón, haz clic en **Escrever** (Redactar), elige tu dirección en **De**
(remitente) y envíalo a tu Gmail. En Gmail, abre el mensaje, haz clic en los
tres puntos y luego en **Mostrar original**: SPF, DKIM y DMARC deben aparecer
como **PASS**. Responde desde Gmail; la respuesta llega a la misma
conversación del buzón.

---

## Día a día

| Quiero… | Haz |
|---|---|
| Una dirección nueva (`compras@`, `github@`…) | `just email-address nueva@tudominio.com "Nombre"` |
| Actualizar el proyecto | `git pull` y después `just deploy` |
| Ver los atajos de teclado | En el buzón, pulsa `?` |
| Ver los logs en tiempo real | `cd mailbox && npx wrangler tail` |
| Ver correos que fallaron al procesarse | `just failures` |
| Reprocesar todo (p. ej., después de una actualización) | `just reprocess --all` |

La papelera y el spam se borran definitivamente 30 días después de llegar
ahí.

## Problemas comunes

| Síntoma | Causa probable y solución |
|---|---|
| `just bootstrap` falla al crear el bucket | R2 no se activó (paso 1.3). |
| El buzón abre pero muestra un error al cargar (y `/api/me` responde "autenticação não configurada") | Falta el paso 4.2 (team y AUD), o el despliegue posterior. |
| Justo después de entrar, el buzón muestra "Sua sessão expirou" (sesión expirada) y no sale de ahí | `--allowed-email` no es el correo de tu cuenta de Cloudflare (ver 4.2). |
| No aparece el inicio de sesión de Access | Access no se activó en el Worker (paso 4.1). |
| Un correo enviado al dominio no llega | La dirección no tiene ruta: ejecuta `just email-address` para ella. Compruébalo con `just email-status`. |
| `email-enable` se rechaza | Null MX o SPF `-all` en el dominio (ver 5.1). |
| Envío rechazado con `E_SENDER_NOT_VERIFIED` | El dominio no se verificó en Email Sending (paso 6.1). |
| Envío rechazado en general | Comprueba que el plan sea Workers Paid (paso 1.6). |
| `just deploy` falla con el código 10089 | Se activaron las métricas (`--metrics on`) sin activar Analytics Engine en el panel. Actívalo o ejecuta `just configure --metrics off`. |
| `just deploy` falla con el código 7403 | Suele ser pasajero. Revisa la sesión con `cd mailbox && npx wrangler whoami` y ejecútalo de nuevo. |

La interfaz del buzón está, por ahora, en portugués (Brasil).
