import { getZebraCopy, type ZebraLocale } from "./locale";

/** All visible overview labels live alongside the other interface translations. */
export function getOverviewCopy(locale: ZebraLocale) {
  return getZebraCopy(locale).entityOverview;
}
