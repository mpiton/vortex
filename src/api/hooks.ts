import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import type { UseQueryOptions } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';

export function useTauriQuery<T>(
  command: string,
  args?: Record<string, unknown>,
  options?: Omit<UseQueryOptions<T, Error>, 'queryFn'>
) {
  return useQuery<T, Error>({
    queryKey: args ? [command, args] : [command],
    queryFn: () => tauriInvoke<T>(command, args),
    ...options,
  });
}

interface UseTauriMutationOptions<TData, TVariables> {
  invalidateKeys?: readonly (readonly unknown[])[];
  onSuccess?: (data: TData, variables: TVariables, context: unknown) => void;
  onError?: (error: Error, variables: TVariables, context: unknown) => void;
}

export function useTauriMutation<
  TData = unknown,
  TVariables extends Record<string, unknown> | void = Record<string, unknown>,
>(command: string, options?: UseTauriMutationOptions<TData, TVariables>) {
  const queryClientInstance = useQueryClient();

  return useMutation<TData, Error, TVariables>({
    mutationFn: (variables) =>
      tauriInvoke<TData>(command, variables as Record<string, unknown> | undefined),
    onSuccess: (data, variables, context) => {
      if (options?.invalidateKeys) {
        for (const key of options.invalidateKeys) {
          queryClientInstance.invalidateQueries({ queryKey: key });
        }
      }
      options?.onSuccess?.(data, variables, context);
    },
    onError: options?.onError,
  });
}
