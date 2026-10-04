import { getZebraBriefWords, getZebraTasks } from '@/lib/zebra/locale';

// Prompts follow eval/bench10x/tasks (F01?F12). They ask; they do not assert fit or access.
export const briefTasks = getZebraTasks('en');
export const briefWords = getZebraBriefWords('en');
export type { BriefTaskId } from '@/lib/zebra/locale';
