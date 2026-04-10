import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SegmentVisualization } from '../SegmentVisualization';
import type { SegmentView } from '@/types/download';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

const segments: SegmentView[] = [
  { id: 0, startByte: 0, endByte: 524288, downloadedBytes: 262144, state: 'Downloading' },
  { id: 1, startByte: 524288, endByte: 1048576, downloadedBytes: 524288, state: 'Completed' },
];

describe('SegmentVisualization', () => {
  it('should render all segments', () => {
    render(<SegmentVisualization segments={segments} totalBytes={1048576} />);
    expect(screen.getByText('Segment 1')).toBeInTheDocument();
    expect(screen.getByText('Segment 2')).toBeInTheDocument();
  });

  it('should handle empty segments array', () => {
    render(<SegmentVisualization segments={[]} totalBytes={1048576} />);
    expect(screen.getByText('No segments')).toBeInTheDocument();
  });

  it('should handle zero totalBytes', () => {
    render(<SegmentVisualization segments={segments} totalBytes={0} />);
    // With totalBytes=0, progress computes to 0%
    const percentTexts = screen.getAllByText('0.0%');
    expect(percentTexts.length).toBeGreaterThan(0);
  });
});
