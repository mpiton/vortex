import { Search } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';

interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
}

export function SearchBar({ value, onChange }: SearchBarProps) {
  const { t } = useTranslation();

  return (
    <div className="relative">
      <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        data-shortcut-target="downloads-search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={t('downloads.searchPlaceholder')}
        aria-label={t('downloads.searchAriaLabel')}
        className="pl-9"
      />
    </div>
  );
}
