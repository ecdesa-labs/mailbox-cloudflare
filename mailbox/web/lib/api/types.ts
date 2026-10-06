export type Id = string; // ULID, 26 caracteres
export type UnixMs = number;
export type Folder = 'inbox' | 'archive' | 'trash' | 'spam' | 'sent';

export interface Person {
  name: string | null;
  addr: string;
}

export interface Me {
  email: string;
}

export interface Identity {
  id: Id;
  address: string;
  label: string | null;
  is_default_from: boolean;
}

export interface Domain {
  id: Id;
  name: string; // "exemplo.com.br"
  label: string; // nome curto exibido
  color: string; // cor do domínio, ex.: "#2563EB"
  unread: number; // não lidos na caixa de entrada
  identities: Identity[];
}

export interface ListItem {
  id: Id;
  thread_id: string | null;
  domain: string; // nome do domínio
  identity: string | null; // endereço pelo qual o email entrou
  from: Person | null;
  subject: string | null;
  snippet: string | null;
  received_at: UnixMs;
  is_read: boolean;
  is_starred: boolean;
  has_attachments: boolean;
  folder: Folder;
  processing: 'full' | 'partial'; // partial = email grande: sem corpo
}

export interface Page {
  items: ListItem[];
  next_cursor: string | null;
}

export interface Attachment {
  id: Id;
  filename: string | null;
  content_type: string;
  size: number;
  content_id: string | null;
  is_inline: boolean;
  url: string;
}

export interface MessageDetail extends ListItem {
  to: Person[];
  cc: Person[];
  date: UnixMs | null; // header Date
  message_id: string | null;
  envelope_to: string;
  size: number;
  text: string | null;
  html: string | null; // já sanitizado no servidor
  has_remote_images: boolean;
  auth: {
    spf: string | null;
    dkim: string | null;
    dmarc: string | null;
  };
  attachments: Attachment[];
  raw_url: string; // .eml original
}

export interface Thread {
  id: Id;
  subject: string | null;
  messages: ListItem[];
}

export interface MessagePatch {
  read?: boolean | null;
  starred?: boolean | null;
  folder?: Folder | null;
}

export type RuleAction = 'move' | 'archive' | 'mark_read' | 'reject';
export interface Rule {
  id: Id;
  priority: number;
  match_address: string;
  action: RuleAction;
  folder: Folder | null;
  enabled: boolean;
}

export interface RuleInput {
  match_address: string;
  action: RuleAction;
  folder?: Folder;
  priority?: number;
  enabled?: boolean;
}

export type RulePatch = Partial<RuleInput>;

/** Destinatário: endereço ou `{ name, addr }`. */
export type Recipient = string | { name?: string | null; addr: string };

/** `POST /api/messages`. Resposta: `in_reply_to` = id da original. */
export interface SendInput {
  in_reply_to?: Id | null;
  from_identity_id: Id;
  to: Recipient[];
  cc?: Recipient[];
  subject: string;
  text: string;
  html?: string | null;
}

export interface SendResult {
  id: Id;
  thread_id: Id | null;
  /** Cópia do enviado: no índice, na fila (aparece em instantes) ou perdida. */
  stored: 'indexed' | 'pending' | 'lost';
}

export interface ApiError {
  error: {
    code:
      | 'bad_request'
      | 'unauthorized'
      | 'forbidden'
      | 'not_found'
      | 'unsupported_media_type'
      | 'internal'
      | 'unavailable'
      | 'rejected'
      | 'rate_limited';
    message: string;
  };
}

export interface ApiErrorWithStatus extends ApiError {
  status: number;
}

export class ApiRequestError extends Error {
  constructor(
    message: string,
    public status: number,
    public code:
      | 'bad_request'
      | 'unauthorized'
      | 'forbidden'
      | 'not_found'
      | 'unsupported_media_type'
      | 'internal'
      | 'unavailable'
      | 'rejected'
      | 'rate_limited' = 'internal'
  ) {
    super(message);
    this.name = 'ApiRequestError';
  }
}
