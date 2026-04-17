import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import type { UseQueryOptions } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { toast } from '@/lib/toast';

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

// Mutation hook for Tauri IPC commands.
//
// Error feedback contract:
// - If `onError` is NOT provided AND `silentError !== true`, the hook
//   automatically surfaces `toast.error(errorMessage?.(err) ?? err.message)`.
// - If `onError` IS provided, the default toast is suppressed — the caller
//   owns the error UX (inline alert, navigate, custom toast, etc.).
// - If `silentError === true`, no default toast fires (use for background
//   polling or transparent retry).
//
// Success feedback is always caller-owned: pass an `onSuccess` that calls
// `toast.success(t('<ns>.toast.<action>'))` with a business-specific label.
interface UseTauriMutationOptions<TData, TVariables> {
  invalidateKeys?: readonly (readonly unknown[])[];
  onMutate?: (variables: TVariables) => unknown | Promise<unknown>;
  onSuccess?: (data: TData, variables: TVariables, context: unknown) => void;
  onError?: (error: Error, variables: TVariables, context: unknown) => void;
  onSettled?: (
    data: TData | undefined,
    error: Error | null,
    variables: TVariables,
    context: unknown,
  ) => void;
  // onError has precedence over silentError. When onError is provided, the
  // default toast is skipped regardless of silentError.
  silentError?: boolean;
  errorMessage?: (err: Error) => string;
}

function resolveErrorMessage(
  error: Error,
  mapper?: (err: Error) => string,
): string {
  if (!mapper) return error.message;
  try {
    return mapper(error);
  } catch {
    return error.message;
  }
}

export function useTauriMutation<
  TData = unknown,
  TVariables extends Record<string, unknown> | void = Record<string, unknown>,
>(command: string, options?: UseTauriMutationOptions<TData, TVariables>) {
  const queryClientInstance = useQueryClient();

  return useMutation<TData, Error, TVariables>({
    mutationFn: (variables) =>
      tauriInvoke<TData>(command, variables as Record<string, unknown> | undefined),
    onMutate: options?.onMutate,
    onSuccess: (data, variables, context) => {
      if (options?.invalidateKeys) {
        for (const key of options.invalidateKeys) {
          queryClientInstance.invalidateQueries({ queryKey: key });
        }
      }
      options?.onSuccess?.(data, variables, context);
    },
    onError: (error, variables, context) => {
      if (options?.onError) {
        options.onError(error, variables, context);
        return;
      }
      if (options?.silentError) return;
      toast.error(resolveErrorMessage(error, options?.errorMessage));
    },
    onSettled: options?.onSettled,
  });
}
