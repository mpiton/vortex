import { render, screen } from '@testing-library/react';
import { Activity } from 'lucide-react';
import { describe, expect, it } from 'vitest';
import { KpiCard } from '../KpiCard';

describe('KpiCard', () => {
  it('renders label, value and optional hint', () => {
    render(<KpiCard label="Total" value="42" hint="last 7 days" icon={Activity} />);
    expect(screen.getByText('Total')).toBeInTheDocument();
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.getByText('last 7 days')).toBeInTheDocument();
  });

  it('omits hint when absent', () => {
    render(<KpiCard label="Files" value="0" />);
    expect(screen.queryByText('last 7 days')).not.toBeInTheDocument();
  });
});
