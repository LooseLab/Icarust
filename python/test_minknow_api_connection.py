from distutils.command.build_scripts import first_line_re
import logging
import grpc
from matplotlib.pyplot import minorticks_off 
from rich.logging import RichHandler
from minknow_api.manager import Manager
from minknow_api import __version__ as mversion
from pathlib import Path
import minknow_api


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
        print(Path().cwd())
        print(minknow_api.GRPC_CHANNEL_OPTIONS)
        logger.info(f"Installed minknow API Version {mversion}")
        if mversion.startswith("5"):
            host = kwargs.get("host", "localhost")
            port = kwargs.get("port", 10000)
            super().__init__(host, port)
        elif mversion[:2] in {"4.2", "4.3", "4.4", "4.5"}:
            print("Version is less than 5.")
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
                    device_connected = connected_position.device.get_calibration(first_channel=1, last_channel=512)
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