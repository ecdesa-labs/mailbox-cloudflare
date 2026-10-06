import {
  useQuery,
  useInfiniteQuery,
  useMutation,
  useQueryClient,
} from '@tanstack/react-query';
import type { Page, ListItem, MessageDetail, Thread, Rule, MessagePatch } from './types';
import { apiClient } from './client';

// Query Keys
export const queryKeys = {
  all: ['api'] as const,
  me: () => [...queryKeys.all, 'me'] as const,
  domains: () => [...queryKeys.all, 'domains'] as const,
  messages: () => [...queryKeys.all, 'messages'] as const,
  message: (id: string) => [...queryKeys.messages(), id] as const,
  threads: () => [...queryKeys.all, 'threads'] as const,
  thread: (id: string) => [...queryKeys.threads(), id] as const,
  rules: () => [...queryKeys.all, 'rules'] as const,
};

// User
export function useMe() {
  return useQuery({
    queryKey: queryKeys.me(),
    queryFn: () => apiClient.getMe(),
    retry: 1,
  });
}

// Domains
export function useDomains() {
  return useQuery({
    queryKey: queryKeys.domains(),
    queryFn: () => apiClient.getDomains(),
    retry: 1,
  });
}

// Messages (Infinite Query para paginação)
export function useMessages(folder?: string, domain?: string, q?: string) {
  return useInfiniteQuery({
    queryKey: [...queryKeys.messages(), { folder, domain, q }],
    queryFn: ({ pageParam = undefined }) =>
      apiClient.getMessages(folder, domain, q, pageParam, 20),
    getNextPageParam: (lastPage: Page) => lastPage.next_cursor,
    initialPageParam: undefined,
    retry: 1,
  });
}

// Single Message
export function useMessage(id: string | null, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.message(id || ''),
    queryFn: () => (id ? apiClient.getMessage(id) : Promise.reject()),
    enabled: !!id && enabled,
    retry: 1,
  });
}

// Thread
export function useThread(id: string | null, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.thread(id || ''),
    queryFn: () => (id ? apiClient.getThread(id) : Promise.reject()),
    enabled: !!id && enabled,
    retry: 1,
  });
}

// Rules
export function useRules() {
  return useQuery({
    queryKey: queryKeys.rules(),
    queryFn: () => apiClient.getRules(),
    retry: 1,
  });
}

// Mutations
export function usePatchMessage() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: MessagePatch }) =>
      apiClient.patchMessage(id, patch),
    onSuccess: (data) => {
      // Invalidate message detail
      queryClient.invalidateQueries({
        queryKey: queryKeys.message(data.id),
      });
      // Invalidate messages lists
      queryClient.invalidateQueries({
        queryKey: queryKeys.messages(),
      });
    },
  });
}

export function useBulkPatchMessages() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ ids, patch }: { ids: string[]; patch: MessagePatch }) =>
      apiClient.bulkPatchMessages(ids, patch),
    onSuccess: () => {
      // Invalidate all messages
      queryClient.invalidateQueries({
        queryKey: queryKeys.messages(),
      });
    },
  });
}

export function useCreateRule() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: any) => apiClient.createRule(data),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.rules(),
      });
    },
  });
}

export function usePatchRule() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: any }) =>
      apiClient.patchRule(id, patch),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.rules(),
      });
    },
  });
}

export function useDeleteRule() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => apiClient.deleteRule(id),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queryKeys.rules(),
      });
    },
  });
}
