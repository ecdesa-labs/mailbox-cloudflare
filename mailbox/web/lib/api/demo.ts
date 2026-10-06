/**
 * Dados de demonstração (`NEXT_PUBLIC_MOCK=demo`): caixa realista para os
 * prints do README. Pessoas, empresas e domínios são fictícios; o email do
 * GitHub reproduz o layout do de criação de conta, com código e link falsos.
 * Para desenvolver, as fixtures de `fixtures.ts` cobrem os casos de borda.
 */
import type { Attachment, Domain, Folder, ListItem, MessageDetail, Person, Rule } from './types';

const now = Date.now();
const min = 60_000;
const hour = 60 * min;
const day = 24 * hour;

export const me = 'ana@pessoal.dev';

export const domains: Domain[] = [
  {
    id: '01M44NRD0000000000000000D1',
    name: 'estudioaurora.com.br',
    label: 'Aurora',
    color: '#2563EB',
    unread: 0,
    identities: [
      { id: '01M44NRD0000000000000000E1', address: 'contato@estudioaurora.com.br', label: 'Estúdio Aurora', is_default_from: true },
      { id: '01M44NRD0000000000000000E2', address: 'financeiro@estudioaurora.com.br', label: 'Financeiro Aurora', is_default_from: false },
    ],
  },
  {
    id: '01M44NRD0000000000000000D2',
    name: 'pessoal.dev',
    label: 'Pessoal',
    color: '#E5484D',
    unread: 0,
    identities: [
      { id: '01M44NRD0000000000000000E3', address: 'ana@pessoal.dev', label: 'Ana Souza', is_default_from: true },
      { id: '01M44NRD0000000000000000E4', address: 'github@pessoal.dev', label: 'Ana Souza', is_default_from: false },
    ],
  },
];

interface Seed {
  n: number;
  thread: string;
  domain: string;
  identity: string;
  from: Person;
  to: Person[];
  cc?: Person[];
  subject: string;
  at: number;
  text: string;
  html?: string;
  read?: boolean;
  starred?: boolean;
  folder?: Folder;
  attachments?: Omit<Attachment, 'id' | 'url'>[];
  auth?: MessageDetail['auth'];
}

const pass = { spf: 'pass', dkim: 'pass', dmarc: 'pass' };
const id = (n: number) => `01M44NRD00000000000000${String(n).padStart(4, '0')}`;
const tid = (s: string) => `01M44NRD0000000000000T${s.padStart(4, '0')}`;

function build(s: Seed): MessageDetail {
  const mid = id(s.n);
  const attachments: Attachment[] = (s.attachments ?? []).map((a, i) => {
    const aid = `${mid.slice(0, 24)}A${i}`;
    return { ...a, id: aid, url: `/api/attachments/${aid}` };
  });
  return {
    id: mid,
    thread_id: tid(s.thread),
    domain: s.domain,
    identity: s.identity,
    from: s.from,
    subject: s.subject,
    snippet: s.text.replace(/\s+/g, ' ').trim().slice(0, 200),
    received_at: s.at,
    is_read: s.read ?? true,
    is_starred: s.starred ?? false,
    has_attachments: attachments.length > 0,
    folder: s.folder ?? 'inbox',
    processing: 'full',
    to: s.to,
    cc: s.cc ?? [],
    date: s.at,
    message_id: `<${mid}@${s.from.addr.split('@')[1]}>`,
    envelope_to: s.identity,
    size: s.text.length + (s.html?.length ?? 0) + attachments.reduce((t, a) => t + a.size, 0),
    text: s.text,
    html: s.html ?? null,
    has_remote_images: !!s.html?.includes('data-remote-src'),
    auth: s.auth ?? (s.folder === 'sent' ? { spf: null, dkim: null, dmarc: null } : pass),
    attachments,
    raw_url: `/api/messages/${mid}/raw`,
  };
}

const aurora = 'estudioaurora.com.br';
const contato = 'contato@estudioaurora.com.br';
const financeiro = 'financeiro@estudioaurora.com.br';
const estudio: Person = { name: 'Estúdio Aurora', addr: contato };
const marina: Person = { name: 'Marina Duarte', addr: 'marina@padariaflor.com.br' };
const rafael: Person = { name: 'Rafael Lima', addr: 'rafael@padariaflor.com.br' };

// Layout do email de criação de conta do GitHub. Já como a API devolve:
// imagens remotas bloqueadas em `data-remote-src` até o usuário liberar.
const GITHUB_HTML = `<html><body style="margin:0;background:#ffffff">
<table width="100%" cellpadding="0" cellspacing="0" style="font-family:-apple-system,'Segoe UI',Helvetica,Arial,sans-serif;color:#24292f">
<tr><td align="center" style="padding:32px 16px 0">
  <svg width="32" height="32" viewBox="0 0 16 16" aria-label="GitHub"><path fill="#24292f" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
  <h1 style="font-size:24px;font-weight:400;margin:16px 0 24px">Here's your GitHub launch code!</h1>
</td></tr>
<tr><td align="center" style="padding:0 16px">
  <table width="100%" cellpadding="0" cellspacing="0" style="max-width:512px;border:1px solid #d0d7de;border-radius:6px">
    <tr><td align="center" style="padding:24px">
      <img data-remote-src="https://github.githubassets.com/assets/launch-rocket-50abbb915633.png" width="100" height="100" alt="Foguete">
      <p style="font-size:16px;margin:24px 0 8px">Continue signing up for GitHub by entering the code below:</p>
      <p style="font-size:40px;letter-spacing:2px;margin:8px 0 24px">52918364</p>
      <a href="https://github.com/" style="display:inline-block;background:#1f883d;color:#ffffff;text-decoration:none;font-size:14px;padding:12px 20px;border-radius:6px">Open GitHub</a>
    </td></tr>
  </table>
</td></tr>
<tr><td align="center" style="padding:24px 16px">
  <table width="100%" cellpadding="0" cellspacing="0" style="max-width:512px">
    <tr><td style="font-size:14px;color:#57606a">
      Not able to enter the code? Paste the following link into your browser:<br>
      <a href="https://github.com/account_verifications/confirm/7c1e2a4b-0d3f-4e5a-9b6c-1f2e3d4c5b6a/52918364" style="color:#0969da">https://github.com/account_verifications/confirm/7c1e2a4b-0d3f-4e5a-9b6c-1f2e3d4c5b6a/52918364</a>
      <hr style="border:0;border-top:1px solid #d0d7de;margin:24px 0">
      <p style="font-size:12px;color:#57606a;margin:0">You're receiving this email because you recently created a new GitHub account. If this wasn't you, please ignore this email.</p>
    </td></tr>
  </table>
</td></tr>
</table></body></html>`;

const NEWSLETTER_HTML = `<html><body style="margin:0;background:#f6f8fa">
<table width="100%" cellpadding="0" cellspacing="0" style="font-family:Georgia,serif;color:#1f2328">
<tr><td align="center" style="padding:32px 16px">
  <table width="100%" cellpadding="0" cellspacing="0" style="max-width:560px;background:#ffffff;border-radius:8px">
    <tr><td style="padding:28px 32px">
      <p style="font-family:Helvetica,Arial,sans-serif;font-size:12px;letter-spacing:2px;color:#8250df;margin:0">DEV SEMANAL · EDIÇÃO 142</p>
      <h1 style="font-size:26px;margin:12px 0">Edge, filas e o fim do servidor de email</h1>
      <p style="font-size:16px;line-height:1.6">Esta semana: como montar uma caixa de email só com serviços gerenciados, por que filas com DLQ salvam seu fim de semana e três ferramentas novas para Rust no WebAssembly.</p>
    </td></tr>
  </table>
</td></tr></table></body></html>`;

const seeds: Seed[] = [
  {
    n: 1, thread: '1', domain: 'pessoal.dev', identity: 'github@pessoal.dev',
    from: { name: 'GitHub', addr: 'noreply@github.com' }, to: [{ name: null, addr: 'github@pessoal.dev' }],
    subject: '🚀 Your GitHub launch code', at: now - 12 * min, read: false, html: GITHUB_HTML,
    text: "Here's your GitHub launch code! Continue signing up for GitHub by entering the code below: 52918364",
  },
  // Conversa com cliente (4 mensagens, a última não lida).
  {
    n: 2, thread: '2', domain: aurora, identity: contato, from: marina, to: [estudio],
    subject: 'Orçamento para o novo site da padaria', at: now - 3 * day,
    text: 'Olá, pessoal do Aurora!\n\nSomos a Padaria Flor, no centro. Queremos um site novo, com cardápio, encomendas pelo WhatsApp e a nossa história. Vocês conseguem mandar um orçamento?\n\nObrigada,\nMarina Duarte',
  },
  {
    n: 3, thread: '2', domain: aurora, identity: contato, from: estudio, to: [marina], folder: 'sent',
    subject: 'Re: Orçamento para o novo site da padaria', at: now - 3 * day + 4 * hour,
    text: 'Oi, Marina!\n\nQue bom receber o contato de vocês. Pela descrição, o projeto fica em 4 semanas: cardápio editável, botão de encomenda e página "Nossa história". Mando a proposta detalhada amanhã cedo.\n\nAbraço,\nEstúdio Aurora',
  },
  {
    n: 4, thread: '2', domain: aurora, identity: contato, from: marina, to: [estudio], cc: [rafael],
    subject: 'Re: Orçamento para o novo site da padaria', at: now - 1 * day,
    text: 'Adoramos a proposta! Podemos começar na segunda? Coloquei o Rafael em cópia: ele cuida das fotos dos produtos.\n\nMarina',
  },
  {
    n: 5, thread: '2', domain: aurora, identity: contato, from: rafael, to: [estudio], cc: [marina],
    subject: 'Re: Orçamento para o novo site da padaria', at: now - 2 * hour, read: false,
    text: 'Bom dia! Seguem as fotos do cardápio em alta resolução. Qualquer coisa, é só chamar.\n\nRafael Lima\nPadaria Flor',
    attachments: [{ filename: 'fotos-cardapio.zip', content_type: 'application/zip', size: 18_400_000, content_id: null, is_inline: false }],
  },
  {
    n: 6, thread: '6', domain: aurora, identity: financeiro, from: { name: 'Prisma Contabilidade', addr: 'nf@prismacontabil.com.br' },
    to: [{ name: null, addr: financeiro }], subject: 'NF-e 1043 emitida: hospedagem de outubro', at: now - 5 * hour, read: false,
    text: 'Olá! A nota fiscal 1043 foi emitida e está em anexo. Vencimento do boleto: dia 10.\n\nPrisma Contabilidade',
    attachments: [{ filename: 'nfe-1043.pdf', content_type: 'application/pdf', size: 184_000, content_id: null, is_inline: false }],
  },
  {
    n: 7, thread: '7', domain: aurora, identity: contato, from: { name: 'Carlos Mendes', addr: 'carlos@cooperativaverde.org' },
    to: [estudio], subject: 'Reunião de alinhamento: quinta, 15h', at: now - 1 * day - 3 * hour, starred: true,
    text: 'Oi! Confirmando nossa reunião de alinhamento na quinta, às 15h, por vídeo. Mando o link na véspera.\n\nCarlos',
  },
  {
    n: 8, thread: '8', domain: 'pessoal.dev', identity: 'ana@pessoal.dev', from: { name: 'Dev Semanal', addr: 'edicao@devsemanal.com.br' },
    to: [{ name: null, addr: 'ana@pessoal.dev' }], subject: 'Edge, filas e o fim do servidor de email', at: now - 2 * day,
    text: 'Esta semana: como montar uma caixa de email só com serviços gerenciados, por que filas com DLQ salvam seu fim de semana e três ferramentas novas para Rust no WebAssembly.',
    html: NEWSLETTER_HTML,
  },
  {
    n: 9, thread: '9', domain: 'pessoal.dev', identity: 'ana@pessoal.dev', from: { name: 'Pousada Mar Azul', addr: 'reservas@pousadamarazul.com.br' },
    to: [{ name: 'Ana Souza', addr: 'ana@pessoal.dev' }], subject: 'Sua reserva está confirmada', at: now - 4 * day,
    text: 'Olá, Ana! Sua reserva de 14 a 17 de novembro está confirmada. Check-in a partir das 14h. Até breve!\n\nPousada Mar Azul',
  },
  {
    n: 10, thread: '10', domain: 'pessoal.dev', identity: 'github@pessoal.dev', from: { name: 'GitHub', addr: 'notifications@github.com' },
    to: [{ name: null, addr: 'github@pessoal.dev' }], subject: '[aurora/site-padaria] Deploy concluído', at: now - 6 * day,
    text: 'O workflow "deploy" terminou com sucesso no branch main.',
  },
  {
    n: 11, thread: '11', domain: aurora, identity: financeiro, from: { name: 'Provedor Nuvem', addr: 'cobranca@provedornuvem.com.br' },
    to: [{ name: null, addr: financeiro }], subject: 'Fatura de setembro paga', at: now - 9 * day,
    text: 'Recebemos o pagamento da fatura de setembro. Obrigado!',
  },
  {
    n: 12, thread: '12', domain: aurora, identity: contato, from: estudio, to: [{ name: 'Carlos Mendes', addr: 'carlos@cooperativaverde.org' }],
    folder: 'sent', subject: 'Proposta: identidade visual da cooperativa', at: now - 8 * day,
    text: 'Oi, Carlos! Segue o resumo da proposta que conversamos. Fico no aguardo.',
  },
  {
    n: 13, thread: '13', domain: aurora, identity: contato, from: { name: 'Prêmios', addr: 'premios@sorteio-imperdivel.biz' },
    to: [{ name: null, addr: contato }], folder: 'spam', read: false, subject: 'VOCÊ GANHOU!!! Resgate agora', at: now - 1 * day,
    text: 'Clique aqui para resgatar seu prêmio.', auth: { spf: 'fail', dkim: null, dmarc: 'fail' },
  },
  {
    n: 14, thread: '14', domain: aurora, identity: contato, from: { name: 'Feira de Design', addr: 'contato@feiradesign.com.br' },
    to: [{ name: null, addr: contato }], folder: 'archive', subject: 'Inscrições abertas para a Feira de Design', at: now - 20 * day,
    text: 'As inscrições para expositores estão abertas até o fim do mês.',
  },
];

const details = seeds.map(build);

export const messages: ListItem[] = details.map(
  ({ to, cc, date, message_id, envelope_to, size, text, html, has_remote_images, auth, attachments, raw_url, ...item }) => item,
);

export const messageDetails: Record<string, MessageDetail> = Object.fromEntries(details.map((d) => [d.id, d]));

export const rules: Rule[] = [];
