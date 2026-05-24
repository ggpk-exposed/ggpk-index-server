import {Container, getContainer} from "@cloudflare/containers";
import {Hono} from "hono";

export class IndexContainer extends Container<Env> {
    defaultPort = 8080;
    sleepAfter = "10m";
}

const app = new Hono<{
    Bindings: Env;
}>();

app.all("*", async (c) => {
    const container = getContainer(c.env.INDEX_CONTAINER);
    const state = await container.getState();
    if (state.status !== "running") {
        await container.startAndWaitForPorts();
    }

    return container.fetch(c.req.raw);
});

export default app;
