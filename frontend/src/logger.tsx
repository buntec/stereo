const APP_PREFIX = "[Stereo]";

const logger = {
  log: (...args: any[]) => console.log(APP_PREFIX, ...args),
  error: (...args: any[]) => console.error(APP_PREFIX, ...args),
  warn: (...args: any[]) => console.warn(APP_PREFIX, ...args),
  info: (...args: any[]) => console.info(APP_PREFIX, ...args),
  debug: (...args: any[]) => console.debug(APP_PREFIX, ...args),
};

export default logger;
