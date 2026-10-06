import { ApiRequestError } from './types';
import type {
  Me,
  Domain,
  ListItem,
  MessageDetail,
  Page,
  Thread,
  Rule,
  MessagePatch,
  RuleInput,
  RulePatch,
} from './types';
import { mockApi } from './mock';

const BASE_URL = '/api';
// Mock apenas quando explicitamente ativado com NEXT_PUBLIC_MOCK === '1'
const USE_MOCK = process.env.NEXT_PUBLIC_MOCK === '1';

class ApiClient {
  private async request<T>(
    method: string,
    path: string,
    body?: unknown
  ): Promise<T> {
    const url = `${BASE_URL}${path}`;
    const options: RequestInit = {
      method,
      credentials: 'same-origin',
    };

    if (body) {
      options.headers = {
        'Content-Type': 'application/json',
      };
      options.body = JSON.stringify(body);
    }

    const res = await fetch(url, options);

    if (!res.ok) {
      let code: ConstructorParameters<typeof ApiRequestError>[2] = res.status === 401 ? 'unauthorized' : res.status >= 500 ? 'unavailable' : 'internal';
      let message = res.status === 401 ? 'Sua sessão expirou' : 'Não foi possível concluir a solicitação.';
      try {
        const data = (await res.json()) as { error?: { code?: typeof code; message?: string } };
        code = data.error?.code ?? code;
        message = data.error?.message ?? message;
      } catch {
        code = 'unavailable';
        message = 'O serviço está temporariamente indisponível.';
      }
      throw new ApiRequestError(message, res.status, code);
    }

    // 204 (ex.: DELETE /api/rules/:id) não tem corpo.
    if (res.status === 204) return undefined as T;
    return res.json();
  }

  // Endpoints públicos
  async getMe(): Promise<Me> {
    if (USE_MOCK) return mockApi.getMe();
    return this.request('GET', '/me');
  }

  async getDomains(): Promise<{ items: Domain[] }> {
    if (USE_MOCK) return mockApi.getDomains();
    return this.request('GET', '/domains');
  }

  async getMessages(
    folder?: string,
    domain?: string,
    q?: string,
    cursor?: string,
    limit?: number
  ): Promise<Page> {
    if (USE_MOCK) return mockApi.getMessages(folder, domain, q, cursor, limit);

    const params = new URLSearchParams();
    if (folder) params.append('folder', folder);
    if (domain) params.append('domain', domain);
    if (q) params.append('q', q);
    if (cursor) params.append('cursor', cursor);
    if (limit) params.append('limit', String(limit));

    const path = `/messages${params.toString() ? `?${params}` : ''}`;
    return this.request('GET', path);
  }

  async getMessage(id: string, remoteImages?: boolean): Promise<MessageDetail> {
    if (USE_MOCK) return mockApi.getMessage(id);

    const params = new URLSearchParams();
    if (remoteImages !== undefined) params.append('remote_images', remoteImages ? '1' : '0');

    const path = `/messages/${id}${params.toString() ? `?${params}` : ''}`;
    return this.request('GET', path);
  }

  async getThread(id: string): Promise<Thread> {
    if (USE_MOCK) return mockApi.getThread(id);
    return this.request('GET', `/threads/${id}`);
  }

  // Mutações
  async patchMessage(id: string, patch: MessagePatch): Promise<ListItem> {
    if (USE_MOCK) return mockApi.patchMessage(id, patch);
    return this.request('PATCH', `/messages/${id}`, patch);
  }

  async bulkPatchMessages(ids: string[], patch: MessagePatch): Promise<{ updated: number }> {
    if (USE_MOCK) return mockApi.bulkPatchMessages(ids, patch);
    return this.request('POST', '/messages/bulk', { ids, patch });
  }

  // Regras
  async getRules(): Promise<{ items: Rule[] }> {
    if (USE_MOCK) return mockApi.getRules();
    return this.request('GET', '/rules');
  }

  async createRule(data: RuleInput): Promise<Rule> {
    if (USE_MOCK) return mockApi.createRule(data);
    return this.request('POST', '/rules', data);
  }

  async patchRule(id: string, patch: RulePatch): Promise<Rule> {
    if (USE_MOCK) return mockApi.patchRule(id, patch);
    return this.request('PATCH', `/rules/${id}`, patch);
  }

  async deleteRule(id: string): Promise<void> {
    if (USE_MOCK) return mockApi.deleteRule(id);
    return this.request('DELETE', `/rules/${id}`);
  }
}

export const apiClient = new ApiClient();
