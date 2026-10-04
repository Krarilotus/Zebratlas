import { getZebraCopy, type ZebraLocale } from "./locale";

export const getModelCopy = (locale: ZebraLocale) => getZebraCopy(locale).modelSettings;
