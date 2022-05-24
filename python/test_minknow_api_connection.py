import logging 
from rich.logging import RichHandler
from minknow_api.manager import Manager
from minknow_api import __version__ as mversion

logger = logging.getLogger(__name__)
logger.addHandler(RichHandler())
logger.setLevel(logging.INFO)

class MinionManager(Manager):
    """
    manager for flowcell positions listed by minKNOW
    """

    def __init__(
        self,
        **kwargs
    ):
        """
        host="localhost",
        port=None,
        use_tls=False,
        Parameters
        ----------
        host: str
            The host that minknow server is being served on
        port: int or None
            Default None. IF None, will be defaulted to 9501, the GRPC port for minKNow
        use_tls: bool
            Default False. Use TLS connection
        """
        logger.info(f"Installed minknow API Version {mversion}")
        if mversion.startswith("5"):
            host = kwargs.get("host", "localhost")
            port = kwargs.get("port", 10000)
            super().__init__(host, port)
        elif mversion[:2] in {"4.2", "4.3", "4.4", "4.5"}:
            host = kwargs.get("host", "localhost")
            port = kwargs.get("port", 9501)
            super().__init__(host, port, use_tls=False)

        self.connected_positions = {}

    def list_devices(self):
        """
        Monitor devices to see if they are active. If active initialise device monitor class for each flow cell position.
        Returns
        -------

        """
        logger.info(list(self.flow_cell_positions()))
        for position in self.flow_cell_positions():
            logger.info(position)
            device_id = position.name
            if device_id not in self.connected_positions:
                running = "yes" if position.running else "no"
                if position.running:
                    connected_position = position.connect()
                    device_connected = connected_position.device.get_device_state().device_state
                    if device_connected:
                        # TODO note that we cannot connect to a remote instance without an ip websocket
                        logger.info("Connected")
                    else:
                        logger.error("Errored")



def main():
    """
    Main function to test connecting to both minknow-5 and minknow-4.5
    """
    mm = MinionManager()
    mm.list_devices()

if __name__ == "__main__":
    main()