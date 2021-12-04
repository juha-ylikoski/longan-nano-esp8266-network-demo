from flask import Flask
from flask import jsonify
import time

import psutil

app = Flask(__name__)


def get_cpu_usage_percent():
    return psutil.cpu_percent()

def get_cpu_temp():
    temps = psutil.sensors_temperatures()["coretemp"]
    current = [int(sensor.current) for sensor in sorted(temps, key=lambda x: x.label)]
    return current

@app.route("/")
def get_data():
    return jsonify(int(get_cpu_usage_percent()), *get_cpu_temp())

app.run(host="0.0.0.0")
