// SPDX-License-Identifier: MIT
import {createRuntime} from "./runtime-adapter.mjs";
import {createDriver} from "./driver.mjs";
const driver = createDriver({loadRuntime:createRuntime, emit:message => self.postMessage(message)});
let queue = Promise.resolve();
self.onmessage = ({data}) => {
  queue = queue.then(() => driver.handle(data)).catch(error => {
    self.postMessage({type:"error", text:error.message});
  });
};
