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
} from './types';
import { domains, messages, messageDetails, rules } from './fixtures';

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
    return { email: 'user@exemplo.com.br' };
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

  async getMessage(id: string): Promise<MessageDetail> {
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

    const threadMessages = messagesState.filter((m) => m.thread_id === id);
    if (threadMessages.length === 0) {
      throw createError(404, 'not_found', 'Thread não encontrada');
    }

    return {
      id,
      subject: threadMessages[0].subject,
      messages: threadMessages,
    };
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
