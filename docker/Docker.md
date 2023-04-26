## Running with Docker

    Or, I see you have common sense

### NB - MinKNOW _must_ still be installed on the system that the docker container is run on!

In order to run with docker, `docker engine > 3.20` and `docker compose` must be installed.

Whilst this is possible on Windows, linux and MacOS, I only have access to one of these OSs, and the volume bindings in the `docker-compose.yml` are linux specific. Pull requests welcome 👀

To run with simple defaults, enter the directory containing your docker-compose file and simply:

```bash
docker compose run icarust
```

Let's break down what happens here. The contents of [docker-compose.yml](docker-compose.yml) are as follows - 

```yaml
version: "3.8"
services:
  icarust:
    image: adoni5/icarust:latest
    init: true
    build:
      context: ..
      dockerfile: ./docker/Dockerfile
    ports:
      - "10000:10000"
      - "10001:10001"
    volumes:
      - ./configs:/configs
      - /opt/ont/minknow/conf/rpc-certs:/opt/ont/minknow/conf/rpc-certs
      - ../squiggle_arrs:/squiggle_arrs

```
Ignoring the `build` section as out of context, (read more [here](https://docs.docker.com/compose/compose-file/build/) if you wish to understand), this file effectively defines the icarust service. 

```yaml
icarust:
  image: adoni5/icarust:latest
  init: true
```


If the docker image for icarust is not found locally, one is pulled from hub.docker.com, from the adoni5/icarust repository. `init:true` means that icarust is run as the top level process, or `PID:1`. This allows it to respond to signals that are sent to the running container, such as `KeyboardInterrupt` to stop the process.

```yaml 
ports:
  - "10000:10000"
  - "10001:10001"
```

This exposes the ports 10000, and 10001 from inside the container to the host machine you are running docker on. The 10000 port is the port that the GRPC manager is running on, and the 10001 port is the port that the fake sequencing positions GRPC server is listening on. 

```yaml
volumes:
  - ./configs:/configs
  - /opt/ont/minknow/conf/rpc-certs:/opt/ont/minknow/conf/rpc-certs
  - ../squiggle_arrs:/squiggle_arrs
```

This final section binds the listed directories on the left of the : of each line to the directories inside the container given on the right. This was the reasoning behind using compose to manage this container as it made the execution command much tidier. 

    - /configs contains the Simulation profile tomls and the config.ini file to pass parameters to the sequencer.
    - /opt/ont/minknow/conf/rpc-certs. This is the one that is hardcoded to linux at the moment. The problem here is that MinKNOW expects TLS secured GRPC connections, so we need to provide this, so MinKNOW must be installe don your system! Or the rpc-certs bundled with MinKNOw must be found at this location.
    - /sqiggle_arrs. Pregenerated squiggle must be placed here. 

So when we call docker compose run, the image listed at `adoni5/icarust:latest` is pulled, the required ports are exposed, and the required volumes are mounted. 

# Pass different arguments to Icarust

The default arguments are defined in the main Icarust repo, in `docker/Dockerfile`. They are set in the Dockerfile, and are

```bash
-v -s /configs/config.toml
```

In order to change the Simulation Profile we are running, simply provide alternatives on the end of the `docker compose run icarust` command. For example: 

```bash
docker compose run icarust -vv -s /configs/<your_config_here>.toml
```

