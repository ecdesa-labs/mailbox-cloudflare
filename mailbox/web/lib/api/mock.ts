import { ApiRequestError } from './types';
import type {
  Domain,
  ListItem,
  MessageDetail,
  Me,
  Page,
  Rule,
  Thread,
  MessagePatch,
  RuleInput,
  RulePatch,
  Recipient,
  SendInput,
  SendResult,
} from './types';
import * as base from './fixtures';
import * as demo from './demo';

// `NEXT_PUBLIC_MOCK=demo`: caixa realista para os prints; `=1`: fixtures de teste.
const data = process.env.NEXT_PUBLIC_MOCK === 'demo' ? demo : { ...base, me: 'user@exemplo.com.br' };
const { domains, messages, messageDetails, rules } = data;

// Simular latência de rede
const ULID_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
function createUlid(): string {
  let value = Date.now();
  let time = '';
  for (let index = 0; index < 10; index += 1) {
    time = ULID_ALPHABET[value % 32] + time;
    value = Math.floor(value / 32);
  }
  let random = '';
  for (let index = 0; index < 16; index += 1) random += ULID_ALPHABET[Math.floor(Math.random() * 32)];
  return time + random;
}

const delay = (ms: number = 300) =>
  new Promise((resolve) => setTimeout(resolve, ms + Math.random() * 300));

// Cópia mutável das fixtures para estado em memória
const messagesState: ListItem[] = structuredClone(messages);
const rulesState: Rule[] = structuredClone(rules);

// Mesmo erro que o cliente real lança, para o tratamento ser igual nos dois modos.
function createError(status: number, code: ApiRequestError['code'], message: string): ApiRequestError {
  return new ApiRequestError(message, status, code);
}

export const mockApi = {
  async getMe(): Promise<Me> {
    await delay();
    return { email: data.me };
  },

  async getDomains(): Promise<{ items: Domain[] }> {
    await delay();
    const items = domains.map((domain) => ({
      ...domain,
      unread: messagesState.filter((message) => message.domain === domain.name && !message.is_read && message.folder === 'inbox').length,
    }));
    return { items };
  },

  async getMessages(
    folder?: string,
    domain?: string,
    q?: string,
    cursor?: string,
    limit: number = 20
  ): Promise<Page> {
    await delay();

    let filtered = [...messagesState];

    // Como a API: sem busca, uma pasta (padrão inbox); com busca e sem
    // folder, todas as pastas.
    if (!q || folder) {
      const f = folder || 'inbox';
      filtered = filtered.filter((m) => m.folder === f);
    }

    if (domain) {
      filtered = filtered.filter((m) => m.domain === domain);
    }

    // Busca por query (busca simples em subject, snippet, from)
    if (q) {
      const query = q.toLowerCase();
      filtered = filtered.filter(
        (m) =>
          (m.subject?.toLowerCase().includes(query)) ||
          (m.snippet?.toLowerCase().includes(query)) ||
          (m.from?.name?.toLowerCase().includes(query)) ||
          (m.from?.addr?.toLowerCase().includes(query))
      );
    }

    // Ordenar por data (mais recente primeiro)
    filtered.sort((a, b) => b.received_at - a.received_at);

    // Implementar paginação com cursor
    let startIdx = 0;
    if (cursor) {
      startIdx = parseInt(cursor, 10);
    }

    const items = filtered.slice(startIdx, startIdx + limit);
    const nextCursor = startIdx + limit < filtered.length ? String(startIdx + limit) : null;

    return { items, next_cursor: nextCursor };
  },

  /** Imagem de exemplo no lugar do anexo (não há servidor no modo mock). */
  async attachmentDataUrl(url: string): Promise<string | null> {
    await delay(100);
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><rect width="200" height="60" rx="8" fill="#2563EB"/><text x="100" y="37" font-family="sans-serif" font-size="16" fill="white" text-anchor="middle">${url.split('/').pop()}</text></svg>`;
    return `data:image/svg+xml;base64,${btoa(svg)}`;
  },

  async getMessage(id: string, remoteImages = false): Promise<MessageDetail> {
    const detail = await this.getMessageBase(id);
    // Como a API: `remote_images=1` devolve data-remote-src como src.
    if (remoteImages && detail.html) {
      return { ...detail, html: detail.html.replace(/data-remote-src=/g, 'src=') };
    }
    return detail;
  },

  async getMessageBase(id: string): Promise<MessageDetail> {
    await delay();

    const detail = messageDetails[id];
    if (detail) {
      // Flags e pasta vêm do estado em memória (refletem as mutações).
      const current = messagesState.find((m) => m.id === id);
      return { ...detail, ...current };
    }

    // Retornar um ListItem convertido em MessageDetail
    const item = messagesState.find((m) => m.id === id);
    if (item) {
      return {
        ...item,
        to: [],
        cc: [],
        date: item.received_at,
        message_id: null,
        envelope_to: item.identity || '',
        size: 0,
        text: item.snippet || null,
        html: null,
        has_remote_images: false,
        auth: { spf: null, dkim: null, dmarc: null },
        attachments: [],
        raw_url: `/api/messages/${id}/raw`,
      };
    }

    throw createError(404, 'not_found', 'Mensagem não encontrada');
  },

  async getThread(id: string): Promise<Thread> {
    await delay();

    // Como a API: mais antiga primeiro; assunto normalizado (sem Re:/Fwd:).
    const threadMessages = messagesState
      .filter((m) => m.thread_id === id)
      .sort((a, b) => a.received_at - b.received_at);
    if (threadMessages.length === 0) {
      throw createError(404, 'not_found', 'Thread não encontrada');
    }

    return {
      id,
      subject: threadMessages[0].subject?.replace(/^\s*((re|fwd?|enc|res)\s*:\s*)+/i, '').toLowerCase() ?? null,
      messages: threadMessages,
    };
  },

  /**
   * Como a API: remetente precisa ser identidade cadastrada; resposta só
   * pelo domínio da original. Para testar erros: destinatário `recusa@...`
   * devolve 422 (recusado pelo serviço) e `falha@...`, 502.
   */
  async sendMessage(input: SendInput): Promise<SendResult> {
    await delay(600);

    const domain = domains.find((d) => d.identities.some((i) => i.id === input.from_identity_id));
    const identity = domain?.identities.find((i) => i.id === input.from_identity_id);
    if (!domain || !identity) throw createError(404, 'not_found', 'identidade não encontrada');
    const original = input.in_reply_to ? messagesState.find((m) => m.id === input.in_reply_to) : undefined;
    if (input.in_reply_to && !original) throw createError(404, 'not_found', 'mensagem original não encontrada');
    if (original && original.domain !== domain.name) {
      throw createError(400, 'bad_request', `from_identity_id: a resposta sai pelo domínio da original (${original.domain})`);
    }
    const people = (list: Recipient[] = []) =>
      list.map((r) => (typeof r === 'string' ? { name: null, addr: r } : { name: r.name ?? null, addr: r.addr }));
    const to = people(input.to);
    const cc = people(input.cc);
    if (to.length === 0) throw createError(400, 'bad_request', 'to: ao menos um destinatário');
    if (to.length + cc.length > 50) throw createError(400, 'bad_request', 'to: no máximo 50 destinatários (to + cc)');
    if ([...to, ...cc].some((p) => p.addr.startsWith('recusa@'))) {
      throw createError(422, 'rejected', 'E_SENDER_NOT_VERIFIED: domínio do remetente não verificado no Email Sending');
    }
    if ([...to, ...cc].some((p) => p.addr.startsWith('falha@'))) {
      throw createError(502, 'unavailable', 'falha no serviço de envio');
    }

    const now = Date.now();
    const item: ListItem = {
      id: createUlid(),
      thread_id: original?.thread_id ?? createUlid(),
      domain: domain.name,
      identity: identity.address,
      from: { name: identity.label, addr: identity.address },
      subject: input.subject,
      snippet: input.text.replace(/\s+/g, ' ').trim().slice(0, 200) || null,
      received_at: now,
      is_read: true,
      is_starred: false,
      has_attachments: false,
      folder: 'sent',
      processing: 'full',
    };
    messagesState.push(item);
    messageDetails[item.id] = {
      ...item,
      to,
      cc,
      date: now,
      message_id: `${item.id}@${domain.name}`,
      envelope_to: identity.address,
      size: input.text.length,
      text: input.text,
      html: null,
      has_remote_images: false,
      auth: { spf: null, dkim: null, dmarc: null },
      attachments: [],
      raw_url: `/api/messages/${item.id}/raw`,
    };
    return { id: item.id, thread_id: item.thread_id, stored: 'indexed' };
  },

  async patchMessage(id: string, patch: MessagePatch): Promise<ListItem> {
    await delay();

    const msg = messagesState.find((m) => m.id === id);
    if (!msg) {
      throw createError(404, 'not_found', 'Mensagem não encontrada');
    }

    if (patch.read !== null && patch.read !== undefined) {
      msg.is_read = patch.read;
    }
    if (patch.starred !== null && patch.starred !== undefined) {
      msg.is_starred = patch.starred;
    }
    if (patch.folder !== null && patch.folder !== undefined) {
      msg.folder = patch.folder;
    }

    return msg;
  },

  async bulkPatchMessages(ids: string[], patch: MessagePatch): Promise<{ updated: number }> {
    await delay();

    let updated = 0;
    for (const id of ids) {
      const msg = messagesState.find((m) => m.id === id);
      if (msg) {
        if (patch.read !== null && patch.read !== undefined) {
          msg.is_read = patch.read;
        }
        if (patch.starred !== null && patch.starred !== undefined) {
          msg.is_starred = patch.starred;
        }
        if (patch.folder !== null && patch.folder !== undefined) {
          msg.folder = patch.folder;
        }
        updated++;
      }
    }

    return { updated };
  },

  async getRules(): Promise<{ items: Rule[] }> {
    await delay();
    return { items: rulesState };
  },

  async createRule(data: RuleInput): Promise<Rule> {
    await delay();

    const rule: Rule = {
      id: createUlid(),
      priority: data.priority ?? 100,
      match_address: data.match_address,
      action: data.action,
      folder: data.folder || null,
      enabled: data.enabled !== false,
    };

    rulesState.push(rule);
    return rule;
  },

  async patchRule(id: string, patch: RulePatch): Promise<Rule> {
    await delay();

    const rule = rulesState.find((r) => r.id === id);
    if (!rule) {
      throw createError(404, 'not_found', 'Regra não encontrada');
    }

    Object.assign(rule, patch);
    return rule;
  },

  async deleteRule(id: string): Promise<void> {
    await delay();

    const idx = rulesState.findIndex((r) => r.id === id);
    if (idx === -1) {
      throw createError(404, 'not_found', 'Regra não encontrada');
    }

    rulesState.splice(idx, 1);
  },
};
