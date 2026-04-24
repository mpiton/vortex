import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useQueryClient } from '@tanstack/react-query';
import { CheckCircle2, XCircle, Loader2 } from 'lucide-react';
import type { DownloadDetailView, VerifyChecksumOutcome } from '@/types/download';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface IntegritySectionProps {
  download: DownloadDetailView;
}

type LocalStatus = 'idle' | 'verifying' | 'verified' | 'mismatch' | 'no-checksum';

function deriveStatus(download: DownloadDetailView): LocalStatus {
  if (!download.checksumExpected) return 'no-checksum';
  if (!download.checksumComputed) return 'idle';
  return download.checksumExpected.trim().toLowerCase()
    === download.checksumComputed.trim().toLowerCase()
    ? 'verified'
    : 'mismatch';
}

function statusLabel(status: LocalStatus): string {
  switch (status) {
    case 'verified':
      return 'Match';
    case 'mismatch':
      return 'Mismatch';
    case 'verifying':
      return 'Verifying…';
    case 'no-checksum':
      return '—';
    case 'idle':
    default:
      return 'Pending';
  }
}

export function IntegritySection({ download }: IntegritySectionProps) {
  const queryClient = useQueryClient();
  const [localStatus, setLocalStatus] = useState<LocalStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const status: LocalStatus = localStatus ?? deriveStatus(download);
  const hasChecksum = download.checksumExpected !== null;
  const algorithm = download.checksumAlgorithm ?? (hasChecksum ? 'SHA-256' : '—');

  async function handleVerify() {
    setError(null);
    setLocalStatus('verifying');
    try {
      const outcome = await invoke<VerifyChecksumOutcome>('download_verify_checksum', {
        id: Number(download.id),
      });
      if (outcome === 'verified') setLocalStatus('verified');
      else if (outcome === 'mismatch') setLocalStatus('mismatch');
      else setLocalStatus('no-checksum');
      // Refresh the detail so checksumComputed propagates from SQLite.
      await queryClient.invalidateQueries({ queryKey: ['download-detail', download.id] });
    } catch (err) {
      setError(String(err));
      setLocalStatus('idle');
    }
  }

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">Integrity</h3>
        {hasChecksum ? (
          <Button
            variant="outline"
            size="sm"
            onClick={handleVerify}
            disabled={status === 'verifying'}
            aria-label="Verify checksum"
          >
            {status === 'verifying' ? (
              <Loader2 className="size-3.5 mr-1 animate-spin" />
            ) : null}
            Verify
          </Button>
        ) : null}
      </div>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">Algorithm</p>
          <p className="font-mono">{algorithm}</p>
        </div>
        <div>
          <p className="text-muted-foreground">Expected Hash</p>
          {hasChecksum ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <p className="font-mono truncate max-w-full cursor-default">
                  {download.checksumExpected}
                </p>
              </TooltipTrigger>
              <TooltipContent>
                <p className="max-w-[400px] break-all">{download.checksumExpected}</p>
              </TooltipContent>
            </Tooltip>
          ) : (
            <p className="font-mono">—</p>
          )}
        </div>
        <div>
          <p className="text-muted-foreground">Computed Hash</p>
          {download.checksumComputed ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <p className="font-mono truncate max-w-full cursor-default">
                  {download.checksumComputed}
                </p>
              </TooltipTrigger>
              <TooltipContent>
                <p className="max-w-[400px] break-all">{download.checksumComputed}</p>
              </TooltipContent>
            </Tooltip>
          ) : (
            <p className="font-mono">—</p>
          )}
        </div>
        <div>
          <p className="text-muted-foreground">Status</p>
          <p
            className="font-mono inline-flex items-center gap-1"
            data-testid="checksum-status"
          >
            {status === 'verified' ? (
              <CheckCircle2 className="size-3.5 text-green-600" aria-hidden="true" />
            ) : null}
            {status === 'mismatch' ? (
              <XCircle className="size-3.5 text-destructive" aria-hidden="true" />
            ) : null}
            {statusLabel(status)}
          </p>
        </div>
        {error ? (
          <p className="text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </section>
  );
}
