export type PackageSourceType =
  | 'container'
  | 'playlist'
  | 'manual'
  | 'split-archive';

export interface PackageView {
  id: string;
  name: string;
  sourceType: string;
  folderPath: string | null;
  autoExtract: boolean;
  priority: number;
  createdAt: number;
  downloadsCount: number;
  totalBytes: number;
  downloadedBytes: number;
  progressPercent: number;
  allCompleted: boolean;
}

export interface PackagePatch {
  name?: string;
  folderPath?: string;
  priority?: number;
  autoExtract?: boolean;
}

export interface PackageListFilter {
  sourceType?: string;
  nameQ?: string;
}

export interface CreatePackageInput {
  name: string;
  sourceType: PackageSourceType;
  folderPath?: string;
}

export interface PackageMoveOutcome {
  moved: number[];
  failed: Array<{
    id: number;
    reason: string;
  }>;
}
