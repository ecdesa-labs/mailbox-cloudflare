// Prints do README (docs/screenshots): navegador Chromium headless pilotado
// pelo protocolo de depuração (WebSocket nativo do Node, sem dependências),
// contra o front em modo demo (NEXT_PUBLIC_MOCK=demo). Use `just screenshots`.
//
//   node scripts/screenshots.mjs <url do front> <pasta de saída> <perfil temporário>
//
// Navegador: $BROWSER ou o primeiro de chromium, google-chrome, brave-browser.
import { execSync, spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';

const [,, base, outDir, profile] = process.argv;
const port = 9333;
mkdirSync(outDir, { recursive: true });
const exe = process.env.BROWSER || ['chromium', 'chromium-browser', 'google-chrome', 'google-chrome-stable', 'brave-browser']
  .find((b) => { try { execSync(`command -v ${b}`, { stdio: 'ignore' }); return true; } catch { return false; } });
if (!exe) throw new Error('nenhum navegador Chromium encontrado (defina BROWSER)');
const browser = spawn(exe, [
  '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
  '--hide-scrollbars', '--no-first-run', '--no-default-browser-check', '--disable-extensions', 'about:blank',
], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let target;
for (let i = 0; i < 50 && !target; i++) {
  try { target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === 'page'); } catch {}
  if (!target) await sleep(200);
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r));
let seq = 0; const pending = new Map();
ws.addEventListener('message', (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } });
const cdp = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++seq; pending.set(id, (m) => (m.error ? reject(new Error(`${method}: ${m.error.message}`)) : resolve(m.result)));
  ws.send(JSON.stringify({ id, method, params }));
});
const js = async (expr) => {
  const r = await cdp('Runtime.evaluate', { expression: `(async () => { ${expr} })()`, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? 'erro no script');
  return r.result.value;
};
const go = async (path, wait = 3000) => { await cdp('Page.navigate', { url: base + path }); await sleep(wait); };
const shot = async (name) => {
  const { data } = await cdp('Page.captureScreenshot', { format: 'png' });
  writeFileSync(`${outDir}/${name}.png`, Buffer.from(data, 'base64'));
  console.log('ok', name);
};
const click = (pred) => js(`const b=[...document.querySelectorAll('button')].find(b=>${pred}); if(!b) throw new Error('botão não achado: ${pred.replace(/'/g, '')}'); b.click(); return true`);

await cdp('Page.enable'); await cdp('Runtime.enable');
// Esconde o indicador do servidor de desenvolvimento do Next (canto inferior esquerdo).
await cdp('Page.addScriptToEvaluateOnNewDocument', { source: `document.addEventListener('DOMContentLoaded', () => { const s = document.createElement('style'); s.textContent = 'nextjs-portal { display: none !important; }'; document.head.appendChild(s); });` });
await cdp('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
await cdp('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'light' }] });
await go('/', 2500);
await js(`localStorage.setItem('mailbox-theme','light'); return true`);

// 1. Caixa de entrada + email do GitHub com as imagens liberadas.
await go('/?id=01M44NRD000000000000000001');
await click(`/Carregar imagens/.test(b.innerText)`);
await sleep(3500);
await js(`document.activeElement?.blur(); return true`);
await shot('caixa-de-entrada');

// 2. Conversa com cliente (a penúltima aberta: ela e a última expandidas).
await go('/?id=01M44NRD000000000000000004', 3500);
// Conversa do topo: título e contagem visíveis.
await js(`document.activeElement?.blur(); document.querySelector('section[aria-label=Leitor] > div:nth-child(2)').scrollTop = 0; return true`);
await sleep(300);
await shot('conversa');

// 3. Respondendo a todos a partir da mensagem do Rafael.
await go('/?id=01M44NRD000000000000000005', 3500);
await click(`b.innerText.trim()==='Responder a todos'`);
await sleep(1200);
await js(`const t=document.querySelector('[role=dialog] textarea'); const set=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set; set.call(t,'Recebemos, Rafael! As fotos ficaram ótimas. Já começamos o cardápio e mandamos uma prévia na sexta.\\n\\nAbraço,\\nEstúdio Aurora'+t.value); t.dispatchEvent(new Event('input',{bubbles:true})); t.setSelectionRange(0,0); return true`);
await sleep(500);
await shot('responder');

// 4. Atalhos de teclado.
await go('/', 3000);
await js(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'?',bubbles:true})); return true`);
await sleep(800);
await shot('atalhos');

// O executável costuma ser um script que lança o navegador: matar o processo
// filho não fecha o navegador (e a porta de depuração ficaria ocupada).
await Promise.race([cdp('Browser.close').catch(() => {}), sleep(2000)]);
ws.close();
